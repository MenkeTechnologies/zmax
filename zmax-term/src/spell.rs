//! Minimal spell checker backing the vim `z=` / `zg` / `zw` / `[s` / `]s` family.
//!
//! The base word list is chosen by vim `spelllang` (default `en` — the system
//! dictionary `/usr/share/dict/words`, present on macOS and most Linux installs;
//! other languages resolve to a hunspell `.dic`). User additions made with `zg`
//! (good) and `zw` (wrong) persist to `<config-dir>/spell-good` and
//! `<config-dir>/spell-bad`; vim `spellfile` moves the good list elsewhere. vim
//! `spellsuggest` caps the `z=` list and can seed it from a file of `bad/good`
//! pairs. If no dictionary is found, nothing is ever flagged misspelled (so the
//! feature degrades to a no-op rather than firing on every word).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};

use zmax_view::DocumentId;

/// How many suggestions `z=` offers when vim `spellsuggest` names no number.
const SUGGEST_MAX: usize = 25;

/// Emacs `flyspell-mode` / `flyspell-prog-mode`: whether a buffer is spell-checked
/// as you type, and over what. Both are buffer-local minor modes in Emacs, so the
/// state is keyed by document.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Flyspell {
    /// Not checking (the default).
    #[default]
    Off,
    /// `flyspell-mode`: check every word in the buffer.
    All,
    /// `flyspell-prog-mode`: check only the prose — comments and string literals.
    Prog,
}

fn flyspell_state() -> &'static RwLock<HashMap<DocumentId, Flyspell>> {
    static S: OnceLock<RwLock<HashMap<DocumentId, Flyspell>>> = OnceLock::new();
    S.get_or_init(|| RwLock::new(HashMap::new()))
}

/// The flyspell state of `doc` — read by the renderer to decide whether (and
/// where) to underline misspellings.
pub fn flyspell(doc: DocumentId) -> Flyspell {
    flyspell_state()
        .read()
        .unwrap()
        .get(&doc)
        .copied()
        .unwrap_or_default()
}

/// Toggle `mode` on `doc`: turning on a mode that is already on turns it off,
/// and switching between `flyspell-mode` and `flyspell-prog-mode` replaces the
/// old one (Emacs's minor modes are mutually exclusive here — the last one
/// enabled owns the buffer). Returns the new state.
pub fn toggle_flyspell(doc: DocumentId, mode: Flyspell) -> Flyspell {
    let mut s = flyspell_state().write().unwrap();
    let cur = s.get(&doc).copied().unwrap_or_default();
    let next = if cur == mode { Flyspell::Off } else { mode };
    if next == Flyspell::Off {
        s.remove(&doc);
    } else {
        s.insert(doc, next);
    }
    next
}

/// vim `spellfile` (`spf`): the word list `zg` adds to. Only the first name of a
/// comma list is used (vim reaches the others with a count before `zg`), and `~`
/// is expanded. Unset falls back to zmax's own `spell-good`.
fn good_path() -> PathBuf {
    match crate::commands::typed::vim_opt_str("spellfile")
        .or_else(|| crate::commands::typed::vim_opt_str("spf"))
    {
        Some(spec) => {
            let first = spec.split(',').next().unwrap_or("").trim().to_string();
            zmax_stdx::path::expand_tilde(Path::new(&first)).into_owned()
        }
        None => zmax_loader::config_dir().join("spell-good"),
    }
}
fn bad_path() -> PathBuf {
    zmax_loader::config_dir().join("spell-bad")
}

/// vim `spelllang` (`spl`, default `en`): the word lists to check against. Each
/// name resolves to the first of these that exists:
///
/// * English (`en…`): the system dictionary (`/usr/share/dict/words`), which is
///   what zmax always used.
/// * `<config-dir>/spell/<name>.dic`, `/usr/share/dict/<name>`,
///   `/usr/share/hunspell/<name>.dic`, `/usr/share/myspell/dicts/<name>.dic`.
///
/// A `.dic` file is a hunspell dictionary: a leading word count, then one word
/// per line with its affix flags after a `/`, which are stripped. Listing more
/// than one language checks against the union of their word lists.
fn dict_paths(lang: &str) -> Vec<PathBuf> {
    let lang = lang.trim();
    if lang.is_empty() || lang == "cjk" {
        return Vec::new();
    }
    if lang.starts_with("en") {
        return ["/usr/share/dict/words", "/usr/share/dict/web2"]
            .iter()
            .map(PathBuf::from)
            .collect();
    }
    vec![
        zmax_loader::config_dir()
            .join("spell")
            .join(format!("{lang}.dic")),
        PathBuf::from(format!("/usr/share/dict/{lang}")),
        PathBuf::from(format!("/usr/share/hunspell/{lang}.dic")),
        PathBuf::from(format!("/usr/share/myspell/dicts/{lang}.dic")),
    ]
}

/// The words of one dictionary file. Hunspell `.dic` files carry a word count on
/// the first line and affix flags after a `/`; both are dropped. Pure — unit
/// tested via [`parse_dict`].
fn parse_dict(contents: &str, hunspell: bool) -> HashSet<String> {
    let mut set = HashSet::new();
    for (i, line) in contents.lines().enumerate() {
        let line = line.trim();
        // The hunspell header is the number of entries.
        if line.is_empty() || (hunspell && i == 0 && line.parse::<usize>().is_ok()) {
            continue;
        }
        let word = line.split(['/', '\t']).next().unwrap_or("").trim();
        if !word.is_empty() {
            set.insert(word.to_lowercase());
        }
    }
    set
}

/// The current `spelllang` value (vim's default is `en`).
fn spelllang() -> String {
    crate::commands::typed::vim_opt_str("spelllang")
        .or_else(|| crate::commands::typed::vim_opt_str("spl"))
        .unwrap_or_else(|| "en".to_string())
}

/// The word list for the current `spelllang`, loaded once per distinct value of
/// the option (so `:set spelllang=de` swaps the dictionary at runtime). The list
/// is shared, not copied — `is_misspelled` runs on every word of every rendered
/// line.
fn dict() -> Arc<HashSet<String>> {
    // A never-loaded cache is distinguished from "loaded, and the language has no
    // dictionary on this box" by the language key, which is never empty once set.
    let cache = dict_cache();
    let lang = spelllang();
    {
        let hit = cache.read().unwrap();
        if hit.0 == lang {
            return Arc::clone(&hit.1);
        }
    }
    let mut set = HashSet::new();
    for name in lang.split(',') {
        for path in dict_paths(name) {
            if let Ok(contents) = std::fs::read_to_string(&path) {
                let hunspell = path.extension().is_some_and(|e| e == "dic");
                set.extend(parse_dict(&contents, hunspell));
                break;
            }
        }
    }
    let set = Arc::new(set);
    *cache.write().unwrap() = (lang, Arc::clone(&set));
    set
}

/// The dictionary cache, so `install_dict` can drop a language whose word list it
/// just rewrote. Shared with [`dict`].
fn dict_cache() -> &'static RwLock<(String, Arc<HashSet<String>>)> {
    static DICT: OnceLock<RwLock<(String, Arc<HashSet<String>>)>> = OnceLock::new();
    DICT.get_or_init(|| RwLock::new((String::new(), Arc::new(HashSet::new()))))
}

/// vim `:mkspell {outname} {infile}` — compile a word list into the dictionary
/// `spelllang` will find. vim's output is a binary `.spl`; zmax's speller reads
/// hunspell/plain word lists (see [`dict_paths`]), so the compiled dictionary is
/// written as `<config-dir>/spell/{name}.dic` — the *first* place `dict_paths`
/// looks — in hunspell's shape (a leading entry count, then one word per line).
/// `:set spelllang={name}` then checks against it.
///
/// `input` is the raw word list: one word per line, `#` comment lines and affix
/// flags after a `/` dropped (vim's `.dic` input format). Returns the file written
/// and how many distinct words it holds.
pub fn install_dict(name: &str, input: &str) -> std::io::Result<(PathBuf, usize)> {
    let uncommented: String = input
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    let mut words: Vec<String> = parse_dict(&uncommented, false).into_iter().collect();
    words.sort();
    let dir = zmax_loader::config_dir().join("spell");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{name}.dic"));
    let body = format!("{}\n{}\n", words.len(), words.join("\n"));
    std::fs::write(&path, body)?;
    // The language may be the one currently loaded — drop the cache so the next
    // `is_misspelled` re-reads the file we just wrote.
    if let Ok(mut c) = dict_cache().write() {
        c.0 = String::new();
    }
    Ok((path, words.len()))
}

fn load_words(path: PathBuf) -> HashSet<String> {
    std::fs::read_to_string(path)
        .map(|c| {
            c.lines()
                .map(|l| l.trim().to_lowercase())
                .filter(|w| !w.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// The user word lists, reloaded whenever the file they live in changes (vim
/// `spellfile`).
fn user_list(
    cell: &'static OnceLock<RwLock<(PathBuf, HashSet<String>)>>,
    path: PathBuf,
) -> &'static RwLock<(PathBuf, HashSet<String>)> {
    let lock = cell.get_or_init(|| RwLock::new((path.clone(), load_words(path.clone()))));
    let stale = lock.read().unwrap().0 != path;
    if stale {
        *lock.write().unwrap() = (path.clone(), load_words(path));
    }
    lock
}

fn user_good() -> &'static RwLock<(PathBuf, HashSet<String>)> {
    static G: OnceLock<RwLock<(PathBuf, HashSet<String>)>> = OnceLock::new();
    user_list(&G, good_path())
}
fn user_bad() -> &'static RwLock<(PathBuf, HashSet<String>)> {
    static B: OnceLock<RwLock<(PathBuf, HashSet<String>)>> = OnceLock::new();
    user_list(&B, bad_path())
}

fn persist(path: PathBuf, set: &HashSet<String>) {
    let mut words: Vec<&String> = set.iter().collect();
    words.sort();
    let body = words
        .iter()
        .map(|w| w.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let _ = std::fs::write(path, body);
}

/// Is `word` worth flagging? Short tokens, anything non-alphabetic, and words in
/// the good list are never flagged; words in the bad list always are.
pub fn is_misspelled(word: &str) -> bool {
    let dict = dict();
    if dict.is_empty() {
        return false;
    }
    let w = word.to_lowercase();
    if w.chars().count() < 2 || !w.chars().all(|c| c.is_alphabetic()) {
        return false;
    }
    if user_bad().read().unwrap().1.contains(&w) {
        return true;
    }
    if user_good().read().unwrap().1.contains(&w) {
        return false;
    }
    !dict.contains(&w)
}

/// `zg`: mark a word as correctly spelled (and forget any prior `zw`). The word
/// is written to vim's `spellfile` when that option is set.
pub fn add_good(word: &str) {
    let w = word.to_lowercase();
    {
        let mut g = user_good().write().unwrap();
        g.1.insert(w.clone());
        let (path, words) = &*g;
        persist(path.clone(), words);
    }
    let mut b = user_bad().write().unwrap();
    if b.1.remove(&w) {
        persist(bad_path(), &b.1);
    }
}

/// `zw`: mark a word as incorrectly spelled.
pub fn add_bad(word: &str) {
    let w = word.to_lowercase();
    {
        let mut b = user_bad().write().unwrap();
        b.1.insert(w.clone());
        persist(bad_path(), &b.1);
    }
    let mut g = user_good().write().unwrap();
    if g.1.remove(&w) {
        let (path, words) = &*g;
        persist(path.clone(), words);
    }
}

/// `zug` / `zuw`: undo a previous `zg`/`zw` for the word.
pub fn remove_user(word: &str) {
    let w = word.to_lowercase();
    let mut g = user_good().write().unwrap();
    if g.1.remove(&w) {
        let (path, words) = &*g;
        persist(path.clone(), words);
    }
    let mut b = user_bad().write().unwrap();
    if b.1.remove(&w) {
        persist(bad_path(), &b.1);
    }
}

/// The user-added good words, sorted (vim `:spelldump` fills a buffer with the
/// known-correct words; this returns the user wordlist added via `zg`/`:spellgood`).
pub fn good_words() -> Vec<String> {
    let g = user_good().read().unwrap();
    let mut words: Vec<String> = g.1.iter().cloned().collect();
    words.sort();
    words
}

/// The user-added bad words, sorted.
pub fn bad_words() -> Vec<String> {
    let b = user_bad().read().unwrap();
    let mut words: Vec<String> = b.1.iter().cloned().collect();
    words.sort();
    words
}

/// vim `spellsuggest` (`sps`): the bare number in the value caps how many
/// suggestions `z=` lists. `None` (the default `best`) keeps zmax's own cap.
/// Pure — unit tested.
fn spellsuggest_limit(value: &str) -> Option<usize> {
    value
        .split(',')
        .find_map(|item| item.trim().parse::<usize>().ok())
}

/// vim `spellsuggest=file:{filename}`: the files listing `bad/good` word pairs,
/// one per line, whose suggestion is offered first for a matching bad word.
/// Pure — unit tested.
fn spellsuggest_files(value: &str) -> Vec<PathBuf> {
    value
        .split(',')
        .filter_map(|item| item.trim().strip_prefix("file:"))
        .map(|p| zmax_stdx::path::expand_tilde(Path::new(p.trim())).into_owned())
        .collect()
}

/// The `good` word a `spellsuggest` file gives for `bad`, if any. The file has
/// two columns separated by a slash (`theribal/terrible`). Pure — unit tested.
fn spellsuggest_file_match(contents: &str, bad: &str) -> Option<String> {
    contents.lines().find_map(|line| {
        let (b, good) = line.trim().split_once('/')?;
        (b.trim().eq_ignore_ascii_case(bad)).then(|| good.trim().to_string())
    })
}

/// `z=`: suggestions for `word` — dictionary words within edit distance 1,
/// preserving the original capitalization style. vim `spellsuggest` caps the
/// list (`sps=best,10`) and can seed it from a `file:` of `bad/good` pairs.
pub fn suggest(word: &str) -> Vec<String> {
    let lower = word.to_lowercase();
    let sps = crate::commands::typed::vim_opt_str("spellsuggest")
        .or_else(|| crate::commands::typed::vim_opt_str("sps"))
        .unwrap_or_default();
    // vim `spellsuggest=file:…`: a listed replacement wins over the internal
    // method and is offered first.
    let mut from_file: Vec<String> = Vec::new();
    for path in spellsuggest_files(&sps) {
        if let Ok(contents) = std::fs::read_to_string(&path) {
            if let Some(good) = spellsuggest_file_match(&contents, &lower) {
                from_file.push(match_case(word, &good));
            }
        }
    }
    let limit = spellsuggest_limit(&sps).unwrap_or(SUGGEST_MAX);
    let dict = dict();
    if dict.is_empty() {
        from_file.truncate(limit);
        return from_file;
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
    // The `spellsuggest` file's replacements come first, then the internal ones.
    out.retain(|w| !from_file.contains(w));
    from_file.extend(out);
    from_file.truncate(limit);
    from_file
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flyspell_toggles_per_buffer_and_modes_are_exclusive() {
        let a = DocumentId::default();
        // Unknown buffers are off, and turning the mode off forgets the entry
        // (so a recycled document id never inherits stale state).
        assert_eq!(flyspell(a), Flyspell::Off);

        assert_eq!(toggle_flyspell(a, Flyspell::All), Flyspell::All);
        assert_eq!(flyspell(a), Flyspell::All);

        // Switching to prog-mode replaces flyspell-mode rather than stacking.
        assert_eq!(toggle_flyspell(a, Flyspell::Prog), Flyspell::Prog);
        assert_eq!(flyspell(a), Flyspell::Prog);

        // Toggling the active mode again turns checking off.
        assert_eq!(toggle_flyspell(a, Flyspell::Prog), Flyspell::Off);
        assert_eq!(flyspell(a), Flyspell::Off);
        assert!(
            !flyspell_state().read().unwrap().contains_key(&a),
            "turning flyspell off must drop the buffer's entry, not park it at Off"
        );
    }

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

    /// vim `spellsuggest`: the bare number caps the `z=` list, `file:` names a
    /// list of `bad/good` replacements, and the method words (`best`, `fast`,
    /// `double`, `timeout:…`) are accepted without changing the internal method.
    #[test]
    fn spellsuggest_value_language() {
        assert_eq!(spellsuggest_limit("best"), None, "no number => zmax's cap");
        assert_eq!(spellsuggest_limit("best,10"), Some(10));
        assert_eq!(spellsuggest_limit("fast,timeout:5000,3"), Some(3));

        assert!(spellsuggest_files("best").is_empty());
        assert_eq!(
            spellsuggest_files("best,file:/tmp/sug.txt"),
            vec![PathBuf::from("/tmp/sug.txt")]
        );

        let file = "theribal/terrible\nrecieve/receive\n";
        assert_eq!(
            spellsuggest_file_match(file, "theribal"),
            Some("terrible".to_string())
        );
        assert_eq!(spellsuggest_file_match(file, "nothere"), None);
    }

    /// vim `spellsuggest=…,N`: `z=` never offers more than N suggestions, and a
    /// `file:` replacement is offered first.
    #[test]
    fn spellsuggest_caps_and_seeds_the_suggestion_list() {
        if dict().is_empty() {
            return; // no system dictionary on this box
        }
        crate::commands::typed::vim_opt_store("spellsuggest", "best,1".to_string());
        assert!(
            suggest("teh").len() <= 1,
            "`:set spellsuggest=best,1` caps z= at one suggestion"
        );

        let dir = std::env::temp_dir().join("zmax-spellsuggest-test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("pairs.txt");
        std::fs::write(&file, "teh/THE_FROM_FILE\n").unwrap();
        crate::commands::typed::vim_opt_store(
            "spellsuggest",
            format!("best,3,file:{}", file.display()),
        );
        let s = suggest("teh");
        assert_eq!(
            s.first().map(String::as_str),
            Some("THE_FROM_FILE"),
            "the `file:` replacement comes first, got {s:?}"
        );
        assert!(s.len() <= 3);

        crate::commands::typed::vim_opt_store("spellsuggest", "best".to_string());
        std::fs::remove_file(&file).ok();
    }

    /// vim `spelllang`: a non-English language resolves to a hunspell `.dic`,
    /// whose entry count header and `/FLAGS` suffixes are not words.
    #[test]
    fn spelllang_resolves_dictionaries_and_parses_hunspell() {
        let words = parse_dict("3\nHaus/A\nBaum\n\nStraße/QN\n", true);
        assert!(words.contains("haus"), "affix flags are stripped");
        assert!(words.contains("baum"));
        assert!(words.contains("straße"));
        assert!(!words.contains("3"), "the count header is not a word");

        // A plain word list keeps its first line.
        let words = parse_dict("3\nalpha\n", false);
        assert!(words.contains("3") && words.contains("alpha"));

        // `en` keeps the system dictionary; another language looks for a `.dic`.
        assert_eq!(
            dict_paths("en_us").first(),
            Some(&PathBuf::from("/usr/share/dict/words"))
        );
        let de = dict_paths("de_DE");
        assert!(
            de.iter().any(|p| p.ends_with("de_DE.dic")),
            "expected a hunspell candidate, got {de:?}"
        );
        assert!(dict_paths("cjk").is_empty(), "cjk excludes East Asian text");
    }

    /// vim `:mkspell`: the compiled word list lands where `spelllang` looks for
    /// it, and `:set spelllang={name}` then checks against exactly those words.
    #[test]
    fn mkspell_installs_a_dictionary_spelllang_finds() {
        let (path, n) = install_dict(
            "zmaxtestlang",
            "# a comment\nzmaxword\nfrobnicate/AB\nzmaxword\n",
        )
        .expect("install_dict writes the dictionary");
        assert_eq!(
            n, 2,
            "the comment, the affix flags and the duplicate are not words"
        );
        assert_eq!(
            dict_paths("zmaxtestlang").first(),
            Some(&path),
            "`:mkspell` must write to the first path `spelllang` tries"
        );

        crate::commands::typed::vim_opt_store("spelllang", "zmaxtestlang".to_string());
        assert!(
            !is_misspelled("frobnicate"),
            "a word from the freshly-compiled dictionary is not misspelled"
        );
        assert!(
            is_misspelled("qqqqzz"),
            "a word outside it still is (the dictionary really is the one in use)"
        );

        crate::commands::typed::vim_opt_store("spelllang", "en".to_string());
        std::fs::remove_file(&path).ok();
    }

    /// vim `spellfile`: `zg` writes to (and reads from) the named file instead of
    /// zmax's own `spell-good`.
    #[test]
    fn spellfile_redirects_the_good_word_list() {
        let dir = std::env::temp_dir().join("zmax-spellfile-test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("en.utf-8.add");
        std::fs::remove_file(&file).ok();

        crate::commands::typed::vim_opt_store("spellfile", file.display().to_string());
        assert_eq!(good_path(), file, "`:set spellfile` picks the word list");

        add_good("zmaxword");
        let written = std::fs::read_to_string(&file).unwrap();
        assert!(
            written.contains("zmaxword"),
            "zg must write to the spellfile, got {written:?}"
        );
        assert!(good_words().contains(&"zmaxword".to_string()));

        // A comma list uses the first name (vim reaches the rest with a count).
        crate::commands::typed::vim_opt_store(
            "spellfile",
            format!("{},{}", file.display(), dir.join("other.add").display()),
        );
        assert_eq!(good_path(), file);

        remove_user("zmaxword");
        crate::commands::typed::vim_opt_store("spellfile", String::new());
        std::fs::remove_file(&file).ok();
    }
}
