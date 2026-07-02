//! Pure, editor-type-free algorithms backing the Dired directory-editor mode
//! (`crate::ui::dired` in the term crate). Everything here is filesystem-free and
//! unit-tested in isolation: the term layer reads the directory into
//! [`DiredEntry`] values, calls these to sort / format / transform them, and
//! renders the result. Prior art: GNU Emacs Dired (sorting `s`, `% R`/`% u`/`% l`
//! name transforms, human-readable sizes).

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
/// `dired-listing-switches` "--group-directories-first" style, which zemacs Dired
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
}
