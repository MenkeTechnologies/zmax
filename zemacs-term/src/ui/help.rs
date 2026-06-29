//! Inline help system — a searchable, scrollable Help browser over **every**
//! command (static + `:`-typable, with their live keybindings) plus curated topic
//! pages. Fuzzy filter on the left, full doc + key + aliases on the right.
//!
//! Open: `SPC h` · `:help` · `?`. Type to search · ↑/↓ or C-n/C-p move ·
//! Tab cycles category · Esc closes.

use std::collections::HashMap;

use tui::buffer::Buffer as Surface;
use zemacs_view::{
    graphics::Rect,
    input::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind},
};

use crate::{
    commands::MappableCommand,
    compositor::{Component, Compositor, Context, Event, EventResult},
};

#[derive(Clone, Copy, PartialEq)]
enum Cat {
    All,
    Commands,
    Keys,
    Topics,
}
const CATS: [(Cat, &str); 4] = [
    (Cat::All, "All"),
    (Cat::Commands, "Commands"),
    (Cat::Keys, "Keybindings"),
    (Cat::Topics, "Topics"),
];

struct Entry {
    cat: Cat,
    title: String,
    keys: Vec<String>,
    aliases: Vec<String>,
    doc: String,
}

/// command name → ["normal: d d", "select: x", …]
fn key_index() -> HashMap<String, Vec<String>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    let km = crate::keymap::default();
    for (mode, trie) in &km {
        let short = match *mode {
            zemacs_view::document::Mode::Normal => "n",
            zemacs_view::document::Mode::Select => "v",
            zemacs_view::document::Mode::Insert => "i",
        };
        for (cmd, chords) in trie.reverse_map() {
            let e = out.entry(cmd).or_default();
            for chord in chords {
                let s = chord
                    .iter()
                    .map(|k| k.to_string())
                    .collect::<Vec<_>>()
                    .join(" ");
                e.push(format!("{short}: {s}"));
            }
        }
    }
    for v in out.values_mut() {
        v.sort();
        v.dedup();
    }
    out
}

const TOPICS: &[(&str, &str)] = &[
    (
        "Welcome to zemacs",
        "zemacs is a hackable modal editor with a full IDE shell (project tree, structure, \
      problems, run window, git, minimap) and a vim-faithful keymap.\n\n\
      • Press SPC (space) for the leader menu.\n\
      • SPC , opens Preferences (Settings, Keymap, Color Scheme, Run Configs).\n\
      • SPC h opens this Help.\n\
      • : opens the command line; SPC SPC is the command palette (M-x).",
    ),
    (
        "Editing modes",
        "Normal — move and operate (h j k l, w b e, d c y, etc.).\n\
      Insert — type text; i a o, I A O, cw, s.  Esc / C-c returns to Normal.\n\
      Visual (Select) — v / V / C-v to select, then an operator (d y c > <).\n\
      gv reselects the last visual area.  Counts: 3dd, 5j, etc.",
    ),
    (
        "Search & replace",
        "/  search forward, ?  search backward, n / N  next / previous.\n\
      *  / #  search the word under the cursor.\n\
      :%s/old/new/g  substitute in the file;  &  repeats the last substitute.\n\
      gn selects the next match (then c / d operates on it).",
    ),
    (
        "Windows, splits & buffers",
        "C-w s / C-w v  horizontal / vertical split.  C-w h/j/k/l move between them.\n\
      C-w > / C-w <  resize width;  C-w + / C-w -  resize height;  C-w =  equalize.\n\
      C-w q  close;  C-w o  only.  ]b / [b  next / previous buffer.",
    ),
    (
        "Folds",
        "za toggle, zo open, zc close, zR open all, zM close all.\n\
      zj / zk move between folds.  zf{motion} creates a fold.",
    ),
    (
        "Spell checking",
        "]s / [s  next / previous misspelled word.\n\
      z=  suggestions (press a number to apply).\n\
      zg  mark good, zw  mark wrong, zug / zuw  undo.  Uses the system dictionary.",
    ),
    (
        "Run configurations",
        "SPC R c  opens the Run/Debug Configurations manager (add/edit/delete named \
      configs).  SPC R r  runs the active config.  The ▶ Run toolbar button runs it too. \
      Configs persist to <workspace>/.zemacs/run-configs.toml.",
    ),
    (
        "Preferences & settings",
        "SPC ,  opens the unified Preferences window:\n\
      • Settings — every editor option, searchable, applied live (no restart).\n\
      • Keymap — add/edit your own [keys.*] bindings.\n\
      • Color Scheme — edit theme colors and save a custom theme.\n\
      • Run Configs — manage run configurations.\n\
      Ctrl-Tab cycles tabs.  Edits live-reload immediately.",
    ),
    (
        "Digraphs & special insert",
        "In insert mode: C-k {c1}{c2} inserts a digraph (e.g. C-k a' → á, C-k -> → →).\n\
      C-v / C-q insert the next key literally.  C-r inserts a register.\n\
      C-e / C-y copy the character below / above the cursor.",
    ),
    (
        "The leader (SPC) menu",
        "SPC is the spacemacs-style leader.  A which-key popup shows the next keys.\n\
      SPC f  files, SPC b  buffers, SPC s  search, SPC g  git, SPC p  project,\n\
      SPC w  windows, SPC R  run, SPC ,  preferences, SPC h  help.",
    ),
    (
        "Preferences (SPC ,)",
        "A full-screen tabbed page — Ctrl-Tab cycles tabs, Esc closes, everything is\n\
      mouse + keyboard and applies live (no restart):\n\
      • Settings — every editor option, searchable.\n\
      • Keymap — your [keys.*] overrides + a browse-all-bindings reference.\n\
      • Color Scheme — theme picker + per-scope color/style editor.\n\
      • Run Configs — named run/debug configurations.\n\
      • Help — this browser.",
    ),
    (
        "Settings tab",
        "Every [editor] option is listed automatically (so nothing is ever missing),\n\
      grouped into sections and searchable with /.\n\
      • Booleans toggle with Space/⏎/click.\n\
      • Enums (line-number, cursor-shape, …) cycle through valid values.\n\
      • Numbers/strings are typed; arrays edit as a TOML literal.\n\
      • ● marks a changed value; press r to reset it to the default.\n\
      • o opens the raw config.toml.  All edits apply live.",
    ),
    (
        "Theme studio (Color Scheme)",
        "Left pane: every installed theme — ⏎/click applies it live (● = active).\n\
      Right pane: per-scope editor.  f / b switch foreground / background,\n\
      type a #rrggbb hex; 1/2/3 toggle bold / italic / dim.  A live preview row\n\
      shows a sample styled with your edits.  n names the theme, s saves it to\n\
      ~/.zemacs/themes/<name>.toml and selects it in the picker.",
    ),
    (
        "Keymap editor",
        "Tab toggles between your overrides and a searchable list of ALL bindings.\n\
      In overrides: a add, d delete, e/⏎ edit (mode · chord · command).\n\
      ⌨ Capture key records a chord by pressing the actual keys (e.g. Ctrl-W H).\n\
      Saves to [keys.<mode>] in config.toml and reloads live.",
    ),
    (
        "Run configurations",
        "SPC R c opens the manager: a add, c copy, d delete, e edit, r run.\n\
      Each config has a name, command, working dir, and KEY=VAL env.\n\
      The active one runs from the ▶ toolbar button or SPC R r, and shows in\n\
      the Run tool window.  Stored in <workspace>/.zemacs/run-configs.toml.",
    ),
    (
        "Marks & jumps",
        "m{a-z} sets a mark, `{a-z} / '{a-z} jumps to it.  `` / '' return to the\n\
      previous jump.  C-o / C-i move back / forward in the jumplist.\n\
      gd goto definition, gr references, gi goto implementation (LSP).",
    ),
    (
        "Macros & registers",
        "q{reg} records a macro, q stops, @{reg} replays, @@ repeats.\n\
      \"{reg} selects a register before y/d/p.  C-r {reg} pastes it in insert.\n\
      The Registers (LOTR) tool window shows every register live.",
    ),
    (
        "Text objects & operators",
        "Operators d c y > < =  combine with motions and text objects:\n\
      diw / ciw word, di( / ci\" inside pair, dap paragraph, dat tag.\n\
      i = inside, a = around.  Counts repeat: 2daw, 3dd.  . repeats the change.",
    ),
];

pub struct HelpPanel {
    entries: Vec<Entry>,
    cat: Cat,
    filter: String,
    sel: usize, // index into the filtered view
    top: usize,
    detail_scroll: u16,
    cat_hits: Vec<(u16, u16, u16, usize)>,
    row_hits: Vec<(u16, u16, u16, usize)>, // maps screen row -> filtered index
}

impl Default for HelpPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl HelpPanel {
    pub fn new() -> Self {
        let keys = key_index();
        let mut entries = Vec::new();
        for c in MappableCommand::STATIC_COMMAND_LIST {
            let name = c.name().to_string();
            entries.push(Entry {
                cat: Cat::Commands,
                keys: keys.get(&name).cloned().unwrap_or_default(),
                title: name,
                aliases: Vec::new(),
                doc: c.doc().to_string(),
            });
        }
        for t in crate::commands::typed::TYPABLE_COMMAND_LIST {
            entries.push(Entry {
                cat: Cat::Commands,
                title: format!(":{}", t.name),
                keys: Vec::new(),
                aliases: t.aliases.iter().map(|a| format!(":{a}")).collect(),
                doc: t.doc.to_string(),
            });
        }
        for (title, body) in TOPICS {
            entries.push(Entry {
                cat: Cat::Topics,
                title: title.to_string(),
                keys: Vec::new(),
                aliases: Vec::new(),
                doc: body.to_string(),
            });
        }
        entries.sort_by_key(|a| a.title.to_lowercase());
        Self {
            entries,
            cat: Cat::All,
            filter: String::new(),
            sel: 0,
            top: 0,
            detail_scroll: 0,
            cat_hits: Vec::new(),
            row_hits: Vec::new(),
        }
    }

    fn matches(&self) -> Vec<usize> {
        let f = self.filter.to_lowercase();
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                let in_cat = match self.cat {
                    Cat::All => true,
                    Cat::Keys => !e.keys.is_empty(),
                    c => e.cat == c,
                };
                in_cat
                    && (f.is_empty()
                        || e.title.to_lowercase().contains(&f)
                        || e.doc.to_lowercase().contains(&f)
                        || e.keys.iter().any(|k| k.to_lowercase().contains(&f)))
            })
            .map(|(i, _)| i)
            .collect()
    }

    fn handle_mouse(&mut self, col: u16, row: u16, kind: MouseEventKind) -> EventResult {
        match kind {
            MouseEventKind::ScrollDown => {
                self.sel += 1;
                return EventResult::Consumed(None);
            }
            MouseEventKind::ScrollUp => {
                self.sel = self.sel.saturating_sub(1);
                return EventResult::Consumed(None);
            }
            MouseEventKind::Down(MouseButton::Left) => {}
            _ => return EventResult::Consumed(None),
        }
        if let Some(&(_, _, _, ci)) = self
            .cat_hits
            .iter()
            .find(|&&(x0, x1, r, _)| row == r && col >= x0 && col < x1)
        {
            self.cat = CATS[ci].0;
            self.sel = 0;
            self.top = 0;
            return EventResult::Consumed(None);
        }
        if let Some(&(_, _, _, pos)) = self
            .row_hits
            .iter()
            .find(|&&(r, x0, x1, _)| row == r && col >= x0 && col < x1)
        {
            self.sel = pos;
            self.detail_scroll = 0;
        }
        EventResult::Consumed(None)
    }
}

impl Component for HelpPanel {
    fn handle_event(&mut self, event: &Event, _cx: &mut Context) -> EventResult {
        let key: KeyEvent = match event {
            Event::Key(k) => *k,
            Event::Mouse(ev) => return self.handle_mouse(ev.column, ev.row, ev.kind),
            _ => return EventResult::Ignored(None),
        };
        let n = self.matches().len();
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => {
                return EventResult::Consumed(Some(Box::new(|c: &mut Compositor, _| {
                    c.pop();
                })))
            }
            KeyCode::Tab => {
                let i = CATS.iter().position(|(c, _)| *c == self.cat).unwrap_or(0);
                self.cat = CATS[(i + 1) % CATS.len()].0;
                self.sel = 0;
                self.top = 0;
            }
            KeyCode::Down | KeyCode::Char('n') | KeyCode::Char('j')
                if ctrl || key.code == KeyCode::Down =>
            {
                if n > 0 {
                    self.sel = (self.sel + 1).min(n - 1);
                    self.detail_scroll = 0;
                }
            }
            KeyCode::Up | KeyCode::Char('p') | KeyCode::Char('k')
                if ctrl || key.code == KeyCode::Up =>
            {
                self.sel = self.sel.saturating_sub(1);
                self.detail_scroll = 0;
            }
            KeyCode::PageDown => self.detail_scroll = self.detail_scroll.saturating_add(5),
            KeyCode::PageUp => self.detail_scroll = self.detail_scroll.saturating_sub(5),
            KeyCode::Backspace => {
                self.filter.pop();
                self.sel = 0;
            }
            KeyCode::Char(c) => {
                self.filter.push(c);
                self.sel = 0;
            }
            _ => {}
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        use crate::ui::rat::{render, to_rat_style};
        use ratatui::style::Modifier as RMod;
        use ratatui::text::{Line, Span};
        use ratatui::widgets::{Paragraph, Wrap};

        self.cat_hits.clear();
        self.row_hits.clear();
        let matched = self.matches();
        if self.sel >= matched.len() {
            self.sel = matched.len().saturating_sub(1);
        }

        let theme = &ctx.editor.theme;
        let bg = to_rat_style(theme.get("ui.background"));
        let text = to_rat_style(theme.get("ui.text"));
        let dim = to_rat_style(theme.get("comment"));
        let border = to_rat_style(theme.get("ui.window"));
        let accent = to_rat_style(theme.get("function")).add_modifier(RMod::BOLD);
        let keyc = to_rat_style(theme.get("keyword"));
        surface.clear_with(area, theme.get("ui.background"));

        surface.clear_with(
            Rect::new(area.x, area.y, area.width, 1),
            theme.get("ui.statusline"),
        );
        render(
            Paragraph::new(Span::styled(" Help ", accent)),
            Rect::new(area.x + 1, area.y, area.width.saturating_sub(1), 1),
            surface,
        );
        let _ = (border, bg);
        let inner = Rect::new(
            area.x + 1,
            area.y + 1,
            area.width.saturating_sub(2),
            area.height.saturating_sub(1),
        );
        if inner.width < 24 || inner.height < 6 {
            return;
        }

        // top: category buttons + search box
        let mut x = inner.x + 1;
        for (i, (c, name)) in CATS.iter().enumerate() {
            let lbl = format!(" {name} ");
            let w = lbl.chars().count() as u16;
            let st = if *c == self.cat {
                text.add_modifier(RMod::REVERSED)
            } else {
                dim
            };
            render(
                Paragraph::new(Span::styled(lbl, st)),
                Rect::new(x, inner.y, w, 1),
                surface,
            );
            self.cat_hits.push((x, x + w, inner.y, i));
            x += w + 1;
        }
        render(
            Paragraph::new(Span::styled(
                format!("  🔍 {}▏  ({} results)", self.filter, matched.len()),
                dim,
            )),
            Rect::new(x + 1, inner.y, inner.x + inner.width - x - 1, 1),
            surface,
        );

        // body split: list | detail
        let list_w = (inner.width * 2 / 5).clamp(16, 44);
        let body_y = inner.y + 2;
        let body_h = inner.height.saturating_sub(3);
        // keep selection in view
        if self.sel < self.top {
            self.top = self.sel;
        } else if self.sel >= self.top + body_h as usize {
            self.top = self.sel + 1 - body_h as usize;
        }
        let last = (self.top + body_h as usize).min(matched.len());
        for (pos, &m) in matched.iter().enumerate().take(last).skip(self.top) {
            let e = &self.entries[m];
            let y = body_y + (pos - self.top) as u16;
            let is_sel = pos == self.sel;
            if is_sel {
                surface.set_style(Rect::new(inner.x, y, list_w, 1), theme.get("ui.selection"));
            }
            let glyph = if e.cat == Cat::Topics {
                "📖 "
            } else {
                "› "
            };
            render(
                Paragraph::new(Span::styled(
                    format!("{glyph}{}", e.title),
                    if is_sel { accent } else { text },
                )),
                Rect::new(inner.x, y, list_w, 1),
                surface,
            );
            self.row_hits.push((y, inner.x, inner.x + list_w, pos));
        }

        // divider
        let dx = inner.x + list_w;
        for y in body_y..body_y + body_h {
            render(
                Paragraph::new(Span::styled("│", dim)),
                Rect::new(dx, y, 1, 1),
                surface,
            );
        }

        // detail
        if let Some(&ei) = matched.get(self.sel) {
            let e = &self.entries[ei];
            let detail_x = dx + 2;
            let detail_w = (inner.x + inner.width).saturating_sub(detail_x);
            let mut lines: Vec<Line> = Vec::new();
            lines.push(Line::from(Span::styled(e.title.clone(), accent)));
            if !e.keys.is_empty() {
                lines.push(Line::from(Span::styled(
                    format!("keys: {}", e.keys.join("   ")),
                    keyc,
                )));
            }
            if !e.aliases.is_empty() {
                lines.push(Line::from(Span::styled(
                    format!("aliases: {}", e.aliases.join(", ")),
                    dim,
                )));
            }
            lines.push(Line::from(""));
            for para in e.doc.split('\n') {
                lines.push(Line::from(Span::styled(para.to_string(), text)));
            }
            let para = Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .scroll((self.detail_scroll, 0));
            render(para, Rect::new(detail_x, body_y, detail_w, body_h), surface);
        }

        render(
            Paragraph::new(Span::styled(" type to search · ↑/↓ or C-n/C-p/C-j/C-k move · Tab category · PgUp/PgDn scroll doc · Esc close", dim)),
            Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1),
            surface,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_indexes_commands_keys_topics() {
        let p = HelpPanel::new();
        let cmds = p.entries.iter().filter(|e| e.cat == Cat::Commands).count();
        let topics = p.entries.iter().filter(|e| e.cat == Cat::Topics).count();
        let with_keys = p.entries.iter().filter(|e| !e.keys.is_empty()).count();
        eprintln!("help: {cmds} commands, {topics} topics, {with_keys} with keybindings");
        assert!(cmds > 200, "expected the full command surface, got {cmds}");
        assert!(topics >= 8);
        assert!(
            with_keys > 50,
            "expected many commands to show keys, got {with_keys}"
        );
    }
}
