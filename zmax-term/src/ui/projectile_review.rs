//! Projectile's reviewable project-wide search and replace.
//!
//! `projectile-replace-review` and `projectile-search-review` do not touch the
//! files first and ask questions later: they gather the matches into a results
//! buffer where each one can be toggled, filtered away, or visited, and only the
//! ones still enabled are written back. This is that buffer — the same keys
//! `projectile-replace-mode-map` binds (`t`/`SPC` toggle, `f` toggle the file,
//! `n`/`p` and `M-n`/`M-p` navigate, `k`/`d` keep/flush matches, `K`/`D`
//! keep/flush files, `c`/`x`/`w` the case, regexp and word switches, `r` set the
//! replacement, `g` re-search, `e` export, `!` or `C-c C-c` apply, `q` quit).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::compositor::{Component, Context, Event, EventResult};

use tui::buffer::Buffer as Surface;
use zmax_view::graphics::Rect;
use zmax_view::input::{KeyCode, KeyModifiers};

/// One match: where it is and the line it sits on.
pub struct Match {
    pub file: PathBuf,
    pub line: usize,
    pub text: String,
    /// Whether applying will rewrite this one — `projectile-replace--toggle`.
    pub enabled: bool,
}

/// The little prompt the filter and replacement keys open inside the buffer.
enum Asking {
    Replacement,
    KeepMatches,
    FlushMatches,
    KeepFiles,
    FlushFiles,
}

pub struct Review {
    root: PathBuf,
    pattern: String,
    /// `None` for `projectile-search-review`, which is read-only.
    replacement: Option<String>,
    regexp: bool,
    case_sensitive: bool,
    word: bool,
    matches: Vec<Match>,
    selected: usize,
    scroll: usize,
    viewport: usize,
    asking: Option<(Asking, String)>,
    status: String,
}

impl Review {
    /// A replace reviewer over `pattern` → `replacement`.
    pub fn replace(root: PathBuf, pattern: String, replacement: String, regexp: bool) -> Self {
        let mut review = Review {
            root,
            pattern,
            replacement: Some(replacement),
            regexp,
            case_sensitive: true,
            word: false,
            matches: Vec::new(),
            selected: 0,
            scroll: 0,
            viewport: 1,
            asking: None,
            status: String::new(),
        };
        review.rescan();
        review
    }

    /// A read-only search reviewer — the same buffer without the write-back.
    pub fn search(root: PathBuf, pattern: String, regexp: bool) -> Self {
        let mut review = Review {
            root,
            pattern,
            replacement: None,
            regexp,
            case_sensitive: true,
            word: false,
            matches: Vec::new(),
            selected: 0,
            scroll: 0,
            viewport: 1,
            asking: None,
            status: String::new(),
        };
        review.rescan();
        review
    }

    /// Gather the matches again — `projectile-replace--refresh`.
    fn rescan(&mut self) {
        self.matches = scan(
            &self.root,
            &self.pattern,
            self.regexp,
            self.case_sensitive,
            self.word,
        );
        self.selected = 0;
        self.scroll = 0;
        self.status = format!("{} match(es)", self.matches.len());
    }

    fn enabled_count(&self) -> usize {
        self.matches.iter().filter(|m| m.enabled).count()
    }

    /// The file of the selected match, if there is one.
    fn selected_file(&self) -> Option<PathBuf> {
        self.matches.get(self.selected).map(|m| m.file.clone())
    }

    fn move_selection(&mut self, delta: isize) {
        if self.matches.is_empty() {
            return;
        }
        let last = self.matches.len() - 1;
        self.selected = match delta {
            d if d < 0 => self.selected.saturating_sub(d.unsigned_abs()),
            d => (self.selected + d as usize).min(last),
        };
    }

    /// Step to the first match of the next (or previous) file —
    /// `projectile-replace--goto-next-file`.
    fn move_file(&mut self, forward: bool) {
        let Some(here) = self.selected_file() else {
            return;
        };
        if forward {
            if let Some(next) = self
                .matches
                .iter()
                .position(|m| m.file != here && self.index_of(m) > self.selected)
            {
                self.selected = next;
            }
        } else if let Some(previous) = self.matches[..self.selected]
            .iter()
            .rposition(|m| m.file != here)
        {
            // Land on the *first* match of that file, as emacs does.
            let file = self.matches[previous].file.clone();
            self.selected = self
                .matches
                .iter()
                .position(|m| m.file == file)
                .unwrap_or(previous);
        }
    }

    fn index_of(&self, target: &Match) -> usize {
        self.matches
            .iter()
            .position(|m| std::ptr::eq(m, target))
            .unwrap_or(0)
    }

    /// `projectile-replace--apply`: rewrite every enabled match, grouped by file.
    /// Returns `(files, replacements)` and the paths that changed.
    fn apply(&self) -> (usize, usize, Vec<PathBuf>, Vec<(PathBuf, String)>) {
        let Some(replacement) = self.replacement.as_deref() else {
            return (0, 0, Vec::new(), Vec::new());
        };
        let Some(re) = self.regex() else {
            return (0, 0, Vec::new(), Vec::new());
        };
        let mut by_file: BTreeMap<&Path, Vec<usize>> = BTreeMap::new();
        for m in self.matches.iter().filter(|m| m.enabled) {
            by_file.entry(m.file.as_path()).or_default().push(m.line);
        }
        let mut changed = Vec::new();
        let mut undo = Vec::new();
        let mut total = 0usize;
        for (file, lines) in by_file {
            let Ok(content) = std::fs::read_to_string(file) else {
                continue;
            };
            let mut out = String::with_capacity(content.len());
            let mut replaced = 0usize;
            for (number, line) in content.lines().enumerate() {
                if lines.contains(&(number + 1)) {
                    replaced += re.find_iter(line).count();
                    out.push_str(&re.replace_all(line, replacement));
                } else {
                    out.push_str(line);
                }
                out.push('\n');
            }
            // Keep a file that did not end in a newline as it was.
            if !content.ends_with('\n') {
                out.pop();
            }
            if out != content && std::fs::write(file, out.as_bytes()).is_ok() {
                undo.push((file.to_path_buf(), content));
                changed.push(file.to_path_buf());
                total += replaced;
            }
        }
        (changed.len(), total, changed, undo)
    }

    /// The compiled pattern, honouring the regexp / case / word switches.
    fn regex(&self) -> Option<regex::Regex> {
        let body = if self.regexp {
            self.pattern.clone()
        } else {
            regex::escape(&self.pattern)
        };
        let body = if self.word {
            format!(r"\b(?:{body})\b")
        } else {
            body
        };
        regex::RegexBuilder::new(&body)
            .case_insensitive(!self.case_sensitive)
            .build()
            .ok()
    }

    /// The rows the buffer shows: a header line per file, then its matches.
    fn rows(&self) -> Vec<(String, Option<usize>)> {
        let mut rows = Vec::new();
        let mut current: Option<&Path> = None;
        for (index, m) in self.matches.iter().enumerate() {
            if current != Some(m.file.as_path()) {
                current = Some(m.file.as_path());
                let shown = m.file.strip_prefix(&self.root).unwrap_or(&m.file);
                rows.push((format!("{}", shown.display()), None));
            }
            let mark = if self.replacement.is_none() {
                " "
            } else if m.enabled {
                "x"
            } else {
                " "
            };
            rows.push((
                format!("  [{mark}] {:>5}: {}", m.line, m.text.trim_end()),
                Some(index),
            ));
        }
        rows
    }

    /// Answer the open mini-prompt.
    fn finish_asking(&mut self, answer: String) {
        let Some((what, _)) = self.asking.take() else {
            return;
        };
        match what {
            Asking::Replacement => {
                self.replacement = Some(answer);
                self.status = "Replacement set".to_string();
            }
            Asking::KeepMatches | Asking::FlushMatches => {
                let Ok(re) = regex::Regex::new(&answer) else {
                    self.status = format!("Invalid regexp: {answer}");
                    return;
                };
                let keep = matches!(what, Asking::KeepMatches);
                let before = self.matches.len();
                self.matches
                    .retain(|m| re.is_match(&m.text) == keep);
                self.selected = self.selected.min(self.matches.len().saturating_sub(1));
                self.status = format!("{} match(es) removed", before - self.matches.len());
            }
            Asking::KeepFiles | Asking::FlushFiles => {
                let Ok(re) = regex::Regex::new(&answer) else {
                    self.status = format!("Invalid regexp: {answer}");
                    return;
                };
                let keep = matches!(what, Asking::KeepFiles);
                let root = self.root.clone();
                let before = self.matches.len();
                self.matches.retain(|m| {
                    let shown = m.file.strip_prefix(&root).unwrap_or(&m.file);
                    re.is_match(&shown.to_string_lossy()) == keep
                });
                self.selected = self.selected.min(self.matches.len().saturating_sub(1));
                self.status = format!("{} match(es) removed", before - self.matches.len());
            }
        }
    }
}

/// Run the search and parse ripgrep's `file:line:text` output.
pub fn scan(
    root: &Path,
    pattern: &str,
    regexp: bool,
    case_sensitive: bool,
    word: bool,
) -> Vec<Match> {
    let mut cmd = std::process::Command::new("rg");
    cmd.arg("--line-number").arg("--no-heading").arg("--color=never");
    if !regexp {
        cmd.arg("-F");
    }
    if !case_sensitive {
        cmd.arg("-i");
    }
    if word {
        cmd.arg("-w");
    }
    cmd.arg("-e").arg(pattern).current_dir(root);
    let Ok(out) = cmd.output() else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let (file, rest) = line.split_once(':')?;
            let (number, text) = rest.split_once(':')?;
            Some(Match {
                file: root.join(file),
                line: number.parse().ok()?,
                text: text.to_string(),
                enabled: true,
            })
        })
        .collect()
}

impl Review {
    /// Set the case switch before the buffer is shown (the dispatch switches).
    pub fn set_case_sensitive(&mut self, on: bool) {
        if self.case_sensitive != on {
            self.case_sensitive = on;
            self.rescan();
        }
    }
}

impl Component for Review {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let Event::Key(key) = event else {
            return EventResult::Ignored(None);
        };
        let key = *key;
        let pop: crate::compositor::Callback =
            Box::new(|compositor: &mut crate::compositor::Compositor, _cx| {
                compositor.pop();
            });

        // The mini-prompt the filter keys open owns the keyboard while it is up.
        if let Some((_, buffer)) = &mut self.asking {
            match key.code {
                KeyCode::Esc => {
                    self.asking = None;
                }
                KeyCode::Enter => {
                    let answer = buffer.clone();
                    self.finish_asking(answer);
                }
                KeyCode::Backspace => {
                    buffer.pop();
                }
                KeyCode::Char(c)
                    if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
                {
                    buffer.push(c);
                }
                _ => {}
            }
            return EventResult::Consumed(None);
        }

        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return EventResult::Consumed(Some(pop)),
            KeyCode::Char('n') if alt => self.move_file(true),
            KeyCode::Char('p') if alt => self.move_file(false),
            KeyCode::Char('n') | KeyCode::Down => self.move_selection(1),
            KeyCode::Char('p') | KeyCode::Up => self.move_selection(-1),
            KeyCode::Char('t') | KeyCode::Char(' ') => {
                if let Some(m) = self.matches.get_mut(self.selected) {
                    m.enabled = !m.enabled;
                }
            }
            KeyCode::Char('f') => {
                if let Some(file) = self.selected_file() {
                    let on = self
                        .matches
                        .iter()
                        .filter(|m| m.file == file)
                        .all(|m| m.enabled);
                    for m in self.matches.iter_mut().filter(|m| m.file == file) {
                        m.enabled = !on;
                    }
                }
            }
            KeyCode::Char('r') => {
                if self.replacement.is_some() {
                    self.asking = Some((Asking::Replacement, String::new()));
                }
            }
            // Before the unguarded `c` below, which would otherwise take
            // ctrl-c too and toggle case sensitivity on it.
            KeyCode::Char('c') if ctrl => {}
            KeyCode::Char('c') => {
                self.case_sensitive = !self.case_sensitive;
                self.rescan();
            }
            KeyCode::Char('x') => {
                self.regexp = !self.regexp;
                self.rescan();
            }
            KeyCode::Char('w') => {
                self.word = !self.word;
                self.rescan();
            }
            KeyCode::Char('k') => self.asking = Some((Asking::KeepMatches, String::new())),
            KeyCode::Char('d') => self.asking = Some((Asking::FlushMatches, String::new())),
            KeyCode::Char('K') => self.asking = Some((Asking::KeepFiles, String::new())),
            KeyCode::Char('D') => self.asking = Some((Asking::FlushFiles, String::new())),
            KeyCode::Char('g') => self.rescan(),
            KeyCode::Char('e') => {
                // `projectile-replace--export`: the enabled matches as text.
                let body: String = self
                    .matches
                    .iter()
                    .filter(|m| m.enabled)
                    .map(|m| {
                        let shown = m.file.strip_prefix(&self.root).unwrap_or(&m.file);
                        format!("{}:{}: {}\n", shown.display(), m.line, m.text.trim_end())
                    })
                    .collect();
                if body.is_empty() {
                    self.status = "Nothing to export".to_string();
                } else {
                    crate::commands::show_text_in_scratch(cx.editor, &body);
                    return EventResult::Consumed(Some(pop));
                }
            }
            KeyCode::Enter => {
                if let Some(m) = self.matches.get(self.selected) {
                    let (file, line) = (m.file.clone(), m.line);
                    return EventResult::Consumed(Some(Box::new(
                        move |compositor: &mut crate::compositor::Compositor, cx: &mut Context| {
                            compositor.pop();
                            if cx
                                .editor
                                .open(&file, zmax_view::editor::Action::Replace)
                                .is_ok()
                            {
                                let (view, doc) = zmax_view::current!(cx.editor);
                                let text = doc.text();
                                let line = line.saturating_sub(1).min(text.len_lines().saturating_sub(1));
                                let pos = text.line_to_char(line);
                                doc.set_selection(view.id, zmax_core::Selection::point(pos));
                            }
                        },
                    )));
                }
            }
            KeyCode::Char('!') => return self.apply_and_report(cx, pop),
            _ => {}
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let text_style = theme.get("ui.text");
        let header_style = theme.get("ui.text.focus");
        let info_style = theme.get("ui.linenr");
        let cursor_style = theme.get("ui.cursor");
        surface.clear_with(area, theme.get("ui.background"));
        if area.height < 4 || area.width < 20 {
            return;
        }
        let title = match &self.replacement {
            Some(replacement) => format!(
                " Replace  {}  →  {}   [{}{}{}]  {} of {} enabled",
                self.pattern,
                replacement,
                if self.regexp { "regexp " } else { "literal " },
                if self.case_sensitive { "case " } else { "nocase " },
                if self.word { "word" } else { "any" },
                self.enabled_count(),
                self.matches.len()
            ),
            None => format!(
                " Search  {}   [{}{}{}]  {} match(es)",
                self.pattern,
                if self.regexp { "regexp " } else { "literal " },
                if self.case_sensitive { "case " } else { "nocase " },
                if self.word { "word" } else { "any" },
                self.matches.len()
            ),
        };
        surface.set_stringn(area.x, area.y, &title, area.width as usize, header_style);
        let hint = if self.replacement.is_some() {
            "t toggle  f file  r replacement  k/d keep/flush  K/D files  c/x/w switches  g re-search  e export  ! apply  q quit"
        } else {
            "n/p move  M-n/M-p file  k/d keep/flush  K/D files  c/x/w switches  g re-search  e export  RET visit  q quit"
        };
        surface.set_stringn(area.x, area.y + 1, hint, area.width as usize, info_style);

        let body_y = area.y + 3;
        let body_h = area.height.saturating_sub(4) as usize;
        self.viewport = body_h.max(1);
        let rows = self.rows();
        // Keep the selected match on screen.
        let selected_row = rows
            .iter()
            .position(|(_, index)| *index == Some(self.selected))
            .unwrap_or(0);
        if selected_row < self.scroll {
            self.scroll = selected_row;
        } else if selected_row >= self.scroll + body_h {
            self.scroll = selected_row + 1 - body_h;
        }
        for (offset, (line, index)) in rows.iter().skip(self.scroll).take(body_h).enumerate() {
            let y = body_y + offset as u16;
            let style = if *index == Some(self.selected) {
                cursor_style
            } else if index.is_none() {
                header_style
            } else {
                text_style
            };
            surface.set_stringn(area.x, y, line, area.width as usize, style);
        }
        let footer = match &self.asking {
            Some((what, buffer)) => {
                let label = match what {
                    Asking::Replacement => "Replacement",
                    Asking::KeepMatches => "Keep matches matching",
                    Asking::FlushMatches => "Flush matches matching",
                    Asking::KeepFiles => "Keep files matching",
                    Asking::FlushFiles => "Flush files matching",
                };
                format!(" {label}: {buffer}")
            }
            None => format!(" {}", self.status),
        };
        surface.set_stringn(
            area.x,
            area.y + area.height - 1,
            &footer,
            area.width as usize,
            info_style,
        );
    }
}

impl Review {
    /// Apply, report what changed, and leave — `!` / `C-c C-c`.
    fn apply_and_report(
        &mut self,
        cx: &mut Context,
        pop: crate::compositor::Callback,
    ) -> EventResult {
        if self.replacement.is_none() {
            self.status = "This is a read-only search review".to_string();
            return EventResult::Consumed(None);
        }
        if self.enabled_count() == 0 {
            self.status = "No matches are enabled".to_string();
            return EventResult::Consumed(None);
        }
        let (files, total, changed, undo) = self.apply();
        crate::commands::projectile::record_replace_undo(undo);
        crate::commands::reload_docs_for_paths(cx.editor, &changed);
        cx.editor.set_status(format!(
            "Replaced {total} occurrence(s) in {files} file(s)"
        ));
        EventResult::Consumed(Some(pop))
    }
}
