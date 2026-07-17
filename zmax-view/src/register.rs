use std::{borrow::Cow, collections::HashMap, iter};

use anyhow::Result;
use arc_swap::access::DynAccess;
use zmax_core::NATIVE_LINE_ENDING;

use crate::{
    clipboard::{ClipboardError, ClipboardProvider, ClipboardType},
    Editor,
};

/// vim `history`: how many `:` command-line / `/` search entries are kept.
/// `0` = unbounded (which is what the registers always were). Set by `:set
/// history=N` in zmax-term, which is where the option store lives.
static HISTORY_LIMIT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub fn set_history_limit(max: usize) {
    HISTORY_LIMIT.store(max, std::sync::atomic::Ordering::Relaxed);
}

pub fn history_limit() -> usize {
    HISTORY_LIMIT.load(std::sync::atomic::Ordering::Relaxed)
}

/// A key-value store for saving sets of values.
///
/// Each register corresponds to a `char`. Most chars can be used to store any set of
/// values but a few chars are "special registers". Special registers have unique
/// behaviors when read or written to:
///
/// * Black hole (`_`): all values read and written are discarded
/// * Selection indices (`#`): index number of each selection starting at 1
/// * Selection contents (`.`)
/// * Document path (`%`): filename of the current buffer
/// * System clipboard (`*`)
/// * Primary clipboard (`+`)
pub struct Registers {
    /// The mapping of register to values.
    /// Values are stored in reverse order when inserted with `Registers::write`.
    /// The order is reversed again in `Registers::read`. This allows us to
    /// efficiently prepend new values in `Registers::push`.
    inner: HashMap<char, Vec<String>>,
    clipboard_provider: Box<dyn DynAccess<ClipboardProvider>>,
    pub last_search_register: char,
}

impl Registers {
    pub fn new(clipboard_provider: Box<dyn DynAccess<ClipboardProvider>>) -> Self {
        Self {
            inner: Default::default(),
            clipboard_provider,
            last_search_register: '/',
        }
    }

    /// Names of the registers that currently hold written values, sorted.
    /// Excludes the special read-only registers (`.`, `%`, `#`, `_`).
    pub fn written(&self) -> Vec<char> {
        let mut names: Vec<char> = self.inner.keys().copied().collect();
        names.sort_unstable();
        names
    }

    pub fn read<'a>(&'a self, name: char, editor: &'a Editor) -> Option<RegisterValues<'a>> {
        match name {
            '_' => Some(RegisterValues::new(iter::empty())),
            '#' => {
                // vim alternate-file register: the name of the previously
                // accessed buffer (the one `` C-^ `` / `:b#` returns to).
                let (view, _) = current_ref!(editor);
                let name = view
                    .docs_access_history
                    .last()
                    .and_then(|&id| editor.document(id))
                    .map(|doc| doc.display_name().into_owned())
                    .unwrap_or_default();
                Some(RegisterValues::new(iter::once(Cow::Owned(name))))
            }
            '.' => {
                // vim last-inserted-text register.
                Some(RegisterValues::new(iter::once(Cow::Owned(
                    editor.last_inserted_text.clone(),
                ))))
            }
            '%' => {
                let path = doc!(editor).display_name();
                Some(RegisterValues::new(iter::once(path)))
            }
            '*' | '+' => Some(read_from_clipboard(
                &self.clipboard_provider.load(),
                self.inner.get(&name),
                match name {
                    '+' => ClipboardType::Clipboard,
                    '*' => ClipboardType::Selection,
                    _ => unreachable!(),
                },
            )),
            _ => self
                .inner
                .get(&name)
                .map(|values| RegisterValues::new(values.iter().map(Cow::from).rev())),
        }
    }

    pub fn write(&mut self, name: char, mut values: Vec<String>) -> Result<()> {
        match name {
            '_' => Ok(()),
            '#' | '.' | '%' => Err(anyhow::anyhow!("Register {name} does not support writing")),
            '*' | '+' => {
                self.clipboard_provider.load().set_contents(
                    &values.join(NATIVE_LINE_ENDING.as_str()),
                    match name {
                        '+' => ClipboardType::Clipboard,
                        '*' => ClipboardType::Selection,
                        _ => unreachable!(),
                    },
                )?;
                values.reverse();
                self.inner.insert(name, values);
                Ok(())
            }
            _ => {
                values.reverse();
                self.inner.insert(name, values);
                Ok(())
            }
        }
    }

    /// Store yanked text with vim's register semantics. `register` is the
    /// explicitly-selected register (`None` for the unnamed default):
    ///   * unnamed default → sets `"` **and** the yank register `0`;
    ///   * a named register `a`-`z` → sets it and mirrors `"` (leaves `0`);
    ///   * `A`-`Z` → appends to the lowercase register and mirrors `"`;
    ///   * `_`/`*`/`+`/read-only registers → written plainly (no mirroring).
    pub fn write_yanked(&mut self, register: Option<char>, values: Vec<String>) -> Result<()> {
        let name = register.unwrap_or('"');
        if !is_vim_distributed(name) {
            return self.write(name, values);
        }
        if name.is_ascii_uppercase() {
            self.append_values(name.to_ascii_lowercase(), &values);
        } else if name != '"' {
            self.write(name, values.clone())?;
        }
        // The unnamed register always mirrors the most recent yank.
        self.write('"', values.clone())?;
        // The yank register `0` is set only when no named register was given.
        if register.is_none() || name == '"' {
            self.write('0', values)?;
        }
        Ok(())
    }

    /// Store deleted/changed text with vim's register semantics. `small` is true
    /// for a delete of less than one line (no newline), which vim routes to the
    /// small-delete register `-` instead of rotating the numbered ring:
    ///   * explicit register → sets it (append for `A`-`Z`) and mirrors `"`;
    ///   * unnamed + small → `-` and `"`;
    ///   * unnamed + linewise/multiline → shifts `1`→`9`, new text into `1`, `"`.
    pub fn write_deleted(
        &mut self,
        register: Option<char>,
        values: Vec<String>,
        small: bool,
    ) -> Result<()> {
        let name = register.unwrap_or('"');
        if !is_vim_distributed(name) {
            return self.write(name, values);
        }
        if let Some(reg) = register.filter(|&r| r != '"') {
            if reg.is_ascii_uppercase() {
                self.append_values(reg.to_ascii_lowercase(), &values);
            } else {
                self.write(reg, values.clone())?;
            }
            return self.write('"', values);
        }
        self.write('"', values.clone())?;
        if small {
            self.write('-', values)?;
        } else {
            // Rotate the numbered delete ring: 8→9, 7→8, …, 1→2, then new→1.
            for i in (1..9).rev() {
                let from = char::from_digit(i, 10).unwrap();
                let to = char::from_digit(i + 1, 10).unwrap();
                if let Some(vals) = self.inner.get(&from).cloned() {
                    self.inner.insert(to, vals);
                }
            }
            self.write('1', values)?;
        }
        Ok(())
    }

    /// Append `values` to a lowercase register (vim `A`-`Z`), preserving logical
    /// order across the reversed internal storage.
    /// Append to a register the way vim's `"A`-`"Z` do: the text is joined *into*
    /// the register's existing value, it does not become a second value. Pushing a
    /// second value instead would leave `"ayy` + `"Ayy` holding two values, and a
    /// single cursor only puts the first — the appended line would vanish.
    ///
    /// Line-ness follows vim (`:h quote_alpha`): appending linewise text to a
    /// charwise register inserts a newline first and makes the result linewise,
    /// and appending charwise text to a linewise register leaves it linewise. A
    /// trailing newline is what marks a value linewise here, so both cases have to
    /// fix one up. Extra incoming values (more selections than the register holds)
    /// are appended as-is, which keeps multi-cursor appends working.
    fn append_values(&mut self, name: char, values: &[String]) {
        let mut logical: Vec<String> = self
            .inner
            .get(&name)
            .map(|stored| stored.iter().rev().cloned().collect())
            .unwrap_or_default();
        for (i, incoming) in values.iter().enumerate() {
            let Some(existing) = logical.get_mut(i) else {
                logical.push(incoming.clone());
                continue;
            };
            let existing_linewise = existing.ends_with('\n');
            if !existing_linewise && !incoming.ends_with('\n') {
                existing.push_str(incoming);
                continue;
            }
            if !existing_linewise && !existing.is_empty() {
                existing.push('\n');
            }
            existing.push_str(incoming);
            if !existing.ends_with('\n') {
                existing.push('\n');
            }
        }
        let _ = self.write(name, logical);
    }

    pub fn push(&mut self, name: char, mut value: String) -> Result<()> {
        match name {
            '_' => Ok(()),
            '#' | '.' | '%' => Err(anyhow::anyhow!("Register {name} does not support pushing")),
            '*' | '+' => {
                let clipboard_type = match name {
                    '+' => ClipboardType::Clipboard,
                    '*' => ClipboardType::Selection,
                    _ => unreachable!(),
                };
                let contents = self
                    .clipboard_provider
                    .load()
                    .get_contents(&clipboard_type)?;
                let saved_values = self.inner.entry(name).or_default();

                if !contents_are_saved(saved_values, &contents) {
                    anyhow::bail!("Failed to push to register {name}: clipboard does not match register contents");
                }

                saved_values.push(value.clone());
                if !contents.is_empty() {
                    value.push_str(NATIVE_LINE_ENDING.as_str());
                }
                value.push_str(&contents);
                self.clipboard_provider
                    .load()
                    .set_contents(&value, clipboard_type)?;

                Ok(())
            }
            _ => {
                let values = self.inner.entry(name).or_default();
                values.push(value);
                // vim `history`: the `:` command-line and `/` search histories keep
                // at most that many entries (oldest dropped first). Other registers
                // are not histories and stay unbounded.
                if matches!(name, ':' | '/') {
                    let max = history_limit();
                    if max > 0 && values.len() > max {
                        values.drain(..values.len() - max);
                    }
                }
                Ok(())
            }
        }
    }

    pub fn first<'a>(&'a self, name: char, editor: &'a Editor) -> Option<Cow<'a, str>> {
        self.read(name, editor).and_then(|mut values| values.next())
    }

    pub fn last<'a>(&'a self, name: char, editor: &'a Editor) -> Option<Cow<'a, str>> {
        self.read(name, editor)
            .and_then(|mut values| values.next_back())
    }

    pub fn iter_preview(&self) -> impl Iterator<Item = (char, &str)> {
        self.inner
            .iter()
            .filter(|(name, _)| !matches!(name, '*' | '+'))
            .map(|(name, values)| {
                let preview = values
                    .last()
                    .and_then(|s| s.lines().next())
                    .unwrap_or("<empty>");

                (*name, preview)
            })
            .chain(
                [
                    ('_', "<empty>"),
                    ('#', "<alternate file>"),
                    ('.', "<last inserted text>"),
                    ('%', "<document path>"),
                    ('+', "<system clipboard>"),
                    ('*', "<primary clipboard>"),
                ]
                .iter()
                .copied(),
            )
    }

    pub fn clear(&mut self) {
        self.clear_clipboard(ClipboardType::Clipboard);
        self.clear_clipboard(ClipboardType::Selection);
        self.inner.clear()
    }

    pub fn remove(&mut self, name: char) -> bool {
        match name {
            '*' | '+' => {
                self.clear_clipboard(match name {
                    '+' => ClipboardType::Clipboard,
                    '*' => ClipboardType::Selection,
                    _ => unreachable!(),
                });
                self.inner.remove(&name);

                true
            }
            '_' | '#' | '.' | '%' => false,
            _ => self.inner.remove(&name).is_some(),
        }
    }

    fn clear_clipboard(&mut self, clipboard_type: ClipboardType) {
        if let Err(err) = self
            .clipboard_provider
            .load()
            .set_contents("", clipboard_type)
        {
            log::error!(
                "Failed to clear {} clipboard: {err}",
                match clipboard_type {
                    ClipboardType::Clipboard => "system",
                    ClipboardType::Selection => "primary",
                }
            )
        }
    }

    pub fn clipboard_provider_name(&self) -> String {
        self.clipboard_provider.load().name().into_owned()
    }
}

/// Whether a register participates in vim's yank/delete auto-distribution
/// (unnamed `"`, small-delete `-`, numbered `0`-`9`, named `a`-`z`/`A`-`Z`).
/// The clipboard (`*`/`+`), black hole (`_`), and read-only special registers
/// (`.`/`#`/`%`/`=`/`:`/`/`) are written plainly with no mirroring.
fn is_vim_distributed(name: char) -> bool {
    name == '"' || name == '-' || name.is_ascii_alphanumeric()
}

fn read_from_clipboard<'a>(
    provider: &ClipboardProvider,
    saved_values: Option<&'a Vec<String>>,
    clipboard_type: ClipboardType,
) -> RegisterValues<'a> {
    match provider.get_contents(&clipboard_type) {
        Ok(contents) => {
            // If we're pasting the same values that we just yanked, re-use
            // the saved values. This allows pasting multiple selections
            // even when yanked to a clipboard.
            let Some(values) = saved_values else {
                return RegisterValues::new(iter::once(contents.into()));
            };

            if contents_are_saved(values, &contents) {
                RegisterValues::new(values.iter().map(Cow::from).rev())
            } else {
                RegisterValues::new(iter::once(contents.into()))
            }
        }
        Err(ClipboardError::ReadingNotSupported) => match saved_values {
            Some(values) => RegisterValues::new(values.iter().map(Cow::from).rev()),
            None => RegisterValues::new(iter::empty()),
        },
        Err(err) => {
            log::error!(
                "Failed to read {} clipboard: {err}",
                match clipboard_type {
                    ClipboardType::Clipboard => "system",
                    ClipboardType::Selection => "primary",
                }
            );

            RegisterValues::new(iter::empty())
        }
    }
}

fn contents_are_saved(saved_values: &[String], mut contents: &str) -> bool {
    let line_ending = NATIVE_LINE_ENDING.as_str();
    let mut values = saved_values.iter().rev();

    match values.next() {
        Some(first) if contents.starts_with(first) => {
            contents = &contents[first.len()..];
        }
        None if contents.is_empty() => return true,
        _ => return false,
    }

    for value in values {
        if contents.starts_with(line_ending) && contents[line_ending.len()..].starts_with(value) {
            contents = &contents[line_ending.len() + value.len()..];
        } else {
            return false;
        }
    }

    true
}

// This is a wrapper of an iterator that is both double ended and exact size,
// and can return either owned or borrowed values. Regular registers can
// return borrowed values while some special registers need to return owned
// values.
pub struct RegisterValues<'a> {
    iter: Box<dyn DoubleEndedExactSizeIterator<Item = Cow<'a, str>> + 'a>,
}

impl<'a> RegisterValues<'a> {
    fn new(
        iter: impl DoubleEndedIterator<Item = Cow<'a, str>>
            + ExactSizeIterator<Item = Cow<'a, str>>
            + 'a,
    ) -> Self {
        Self {
            iter: Box::new(iter),
        }
    }
}

impl<'a> Iterator for RegisterValues<'a> {
    type Item = Cow<'a, str>;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl DoubleEndedIterator for RegisterValues<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.iter.next_back()
    }
}

impl ExactSizeIterator for RegisterValues<'_> {
    fn len(&self) -> usize {
        self.iter.len()
    }
}

// Each RegisterValues iterator is both double ended and exact size. We can't
// type RegisterValues as `Box<dyn DoubleEndedIterator + ExactSizeIterator>`
// because only one non-auto trait is allowed in trait objects. So we need to
// create a new trait that covers both. `RegisterValues` wraps that type so that
// trait only needs to live in this module and not be imported for all register
// callsites.
trait DoubleEndedExactSizeIterator: DoubleEndedIterator + ExactSizeIterator {}

impl<I: DoubleEndedIterator + ExactSizeIterator> DoubleEndedExactSizeIterator for I {}

#[cfg(test)]
mod tests {
    use super::*;
    use arc_swap::access::Constant;

    fn registers() -> Registers {
        Registers::new(Box::new(Constant(ClipboardProvider::None)))
    }

    /// Logical (un-reversed) contents of a plain stored register.
    fn logical(regs: &Registers, name: char) -> Vec<String> {
        regs.inner
            .get(&name)
            .map(|v| v.iter().rev().cloned().collect())
            .unwrap_or_default()
    }

    fn v(s: &str) -> Vec<String> {
        vec![s.to_string()]
    }

    /// vim `history=N`: the `:` and `/` histories keep only the N newest entries
    /// (oldest dropped), while an ordinary register stays unbounded.
    #[test]
    fn history_limit_trims_colon_and_search_registers_only() {
        set_history_limit(3);
        let mut r = registers();
        for cmd in ["one", "two", "three", "four", "five"] {
            r.push(':', cmd.to_string()).unwrap();
            r.push('a', cmd.to_string()).unwrap();
        }
        // `logical` reverses: newest first, as `read` yields them.
        assert_eq!(
            logical(&r, ':'),
            vec!["five".to_string(), "four".into(), "three".into()]
        );
        assert_eq!(logical(&r, 'a').len(), 5);
        set_history_limit(0);
    }

    #[test]
    fn yank_to_default_fills_unnamed_and_yank_register() {
        let mut r = registers();
        r.write_yanked(None, v("hello")).unwrap();
        assert_eq!(logical(&r, '"'), v("hello"));
        assert_eq!(logical(&r, '0'), v("hello"));
    }

    #[test]
    fn yank_to_named_register_mirrors_unnamed_but_not_zero() {
        let mut r = registers();
        r.write_yanked(None, v("first")).unwrap(); // sets 0
        r.write_yanked(Some('a'), v("second")).unwrap();
        assert_eq!(logical(&r, 'a'), v("second"));
        assert_eq!(logical(&r, '"'), v("second"));
        // `0` is untouched by a named yank.
        assert_eq!(logical(&r, '0'), v("first"));
    }

    #[test]
    fn uppercase_register_appends_to_lowercase() {
        let mut r = registers();
        r.write_yanked(Some('a'), v("foo")).unwrap();
        r.write_yanked(Some('A'), v("bar")).unwrap();
        // vim has no multi-value registers: `"ayw` then `"Ayw` leaves `a` holding
        // "foobar". Two values would mean a single cursor puts only "foo" and the
        // appended text is lost, which is the bug this asserts against.
        assert_eq!(logical(&r, 'a'), v("foobar"));
    }

    #[test]
    fn uppercase_append_keeps_vim_line_ness() {
        // Measured against nvim -u NONE -i NONE; see the four `"A` cases.
        let mut r = registers();
        r.write_yanked(Some('a'), v("alpha \n")).unwrap();
        r.write_yanked(Some('A'), v("bravo two\n")).unwrap();
        assert_eq!(logical(&r, 'a'), v("alpha \nbravo two\n")); // line+line

        // charwise + linewise: a newline is inserted, result is linewise.
        let mut r = registers();
        r.write_yanked(Some('a'), v("alpha ")).unwrap();
        r.write_yanked(Some('A'), v("bravo two\n")).unwrap();
        assert_eq!(logical(&r, 'a'), v("alpha \nbravo two\n"));

        // linewise + charwise: stays linewise, so it still puts as whole lines.
        let mut r = registers();
        r.write_yanked(Some('a'), v("alpha one\n")).unwrap();
        r.write_yanked(Some('A'), v("bravo ")).unwrap();
        assert_eq!(logical(&r, 'a'), v("alpha one\nbravo \n"));
    }

    #[test]
    fn linewise_delete_rotates_numbered_ring() {
        let mut r = registers();
        r.write_deleted(None, v("one\n"), false).unwrap();
        assert_eq!(logical(&r, '1'), v("one\n"));
        r.write_deleted(None, v("two\n"), false).unwrap();
        assert_eq!(logical(&r, '1'), v("two\n"));
        assert_eq!(logical(&r, '2'), v("one\n")); // shifted down
        assert_eq!(logical(&r, '"'), v("two\n")); // unnamed mirrors latest
    }

    #[test]
    fn small_delete_uses_dash_register_not_ring() {
        let mut r = registers();
        r.write_deleted(None, v("word"), true).unwrap();
        assert_eq!(logical(&r, '-'), v("word"));
        assert!(logical(&r, '1').is_empty()); // ring untouched by a small delete
        assert_eq!(logical(&r, '"'), v("word"));
    }

    #[test]
    fn delete_to_named_register_mirrors_unnamed_and_skips_ring() {
        let mut r = registers();
        r.write_deleted(Some('z'), v("text\n"), false).unwrap();
        assert_eq!(logical(&r, 'z'), v("text\n"));
        assert_eq!(logical(&r, '"'), v("text\n"));
        assert!(logical(&r, '1').is_empty());
    }

    #[test]
    fn ring_rotation_drops_off_the_end_at_nine() {
        let mut r = registers();
        for i in 0..10 {
            r.write_deleted(None, v(&format!("d{i}\n")), false).unwrap();
        }
        // Newest in 1, oldest surviving (d1) in 9; d0 fell off.
        assert_eq!(logical(&r, '1'), v("d9\n"));
        assert_eq!(logical(&r, '9'), v("d1\n"));
    }
}
