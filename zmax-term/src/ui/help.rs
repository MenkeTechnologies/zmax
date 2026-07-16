//! Inline help system — a searchable, scrollable Help browser over **every**
//! command (static + `:`-typable, with their live keybindings) plus curated topic
//! pages. Fuzzy filter on the left, full doc + key + aliases on the right.
//!
//! Open: `SPC h` · `:help` · `?`. Type to search · ↑/↓ or C-n/C-p move ·
//! Tab cycles category · Esc closes.
//!
//! `RET` visits the entry at point — the cross-reference follow (`help-follow`)
//! that pushes onto the help history. While a single entry is displayed (the
//! read-only `*Help*` buffer), Emacs's Help-mode keys are live: `l` / `C-c C-b`
//! go back, `r` / `C-c C-f` go forward, `n` / `p` scroll to the next / previous
//! page of the topic. Any other character leaves the topic and searches again.

use std::collections::HashMap;

use tui::buffer::Buffer as Surface;
use zmax_view::{
    graphics::Rect,
    input::{KeyCode, KeyEvent, MouseButton, MouseEventKind},
};

use crate::{
    commands::MappableCommand,
    compositor::{Component, Compositor, Context, Event, EventResult},
    ctrl, key, shift,
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
            zmax_view::document::Mode::Normal => "n",
            zmax_view::document::Mode::Select => "v",
            zmax_view::document::Mode::Insert => "i",
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
        "Welcome to zmax",
        "zmax is a hackable modal editor with a full IDE shell (project tree, structure, \
      problems, run window, git, minimap) and a vim-faithful keymap.\n\n\
      • Four keymap presets: spacemacs (default), vim, helix, emacs.  Switch with\n\
        :keymap <name> or in Preferences ▸ Keymap.\n\
      • Press SPC (space) for the leader menu; press C-x for the emacs prefix.\n\
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
      :replace-word NEW  global-replace the word under the cursor with NEW.\n\
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
      Configs persist to <workspace>/.zmax/run-configs.toml.",
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
        "In the spacemacs keymap (default) SPC is the leader.  A which-key popup shows\n\
      the next keys.  SPC f  files, SPC b  buffers, SPC s  search, SPC g  git,\n\
      SPC p  project, SPC w  windows, SPC R  run, SPC ,  preferences, SPC h  help.\n\
      (The pure vim keymap has no SPC leader and shows no which-key popup.)",
    ),
    (
        "The C-x prefix (emacs/spacemacs)",
        "In the spacemacs (default) and emacs keymaps, C-x is the Emacs command prefix\n\
      and opens a which-key popup.  C-x C-s save, C-x C-f find-file, C-x b buffer,\n\
      C-x k kill-buffer, C-x o other-window, C-x 0/1/2/3 windows, C-x r registers/\n\
      rectangles/bookmarks, C-x C-c quit.  (In the vim keymap C-x is decrement.)",
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
      ~/.zmax/themes/<name>.toml and selects it in the picker.",
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
      the Run tool window.  Stored in <workspace>/.zmax/run-configs.toml.",
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
    /// The entry being *displayed on its own* (Emacs's one-topic `*Help*`
    /// buffer): while set, the list shows only it. Enter visits the selected
    /// entry; typing or Backspace goes back to browsing.
    visiting: Option<usize>,
    /// Visit history — the entry indices `help-go-back` / `help-go-forward` walk,
    /// oldest first, with `hpos` the position in it.
    history: Vec<usize>,
    hpos: usize,
    /// Height of the detail pane at the last render, so a page scroll moves by a
    /// screenful (Emacs `help-goto-next-page`).
    page: u16,
    /// `C-c` was typed: the panel is waiting for the second key of Emacs's
    /// `C-c C-b` (`help-go-back`) / `C-c C-f` (`help-go-forward`) chords.
    pending_ctrl_c: bool,
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
            visiting: None,
            history: Vec::new(),
            hpos: 0,
            page: 5,
            pending_ctrl_c: false,
        }
    }

    /// Display `entry` on its own and record the visit, so `help-go-back` can
    /// return to whatever was shown before it.
    fn visit(&mut self, entry: usize) {
        if self.visiting == Some(entry) {
            return;
        }
        // A new visit truncates the forward history, exactly as a browser does.
        if !self.history.is_empty() && self.hpos + 1 < self.history.len() {
            self.history.truncate(self.hpos + 1);
        }
        if self.history.last() != Some(&entry) {
            self.history.push(entry);
        }
        self.hpos = self.history.len() - 1;
        self.visiting = Some(entry);
        self.detail_scroll = 0;
    }

    /// Emacs `help-go-back` (`C-c C-b` / `l` in `*Help*`): show the previously
    /// visited help entry. `false` when there is none.
    pub fn go_back(&mut self) -> bool {
        if self.hpos == 0 || self.history.is_empty() {
            return false;
        }
        self.hpos -= 1;
        self.visiting = Some(self.history[self.hpos]);
        self.detail_scroll = 0;
        true
    }

    /// Emacs `help-go-forward` (`C-c C-f` / `r` in `*Help*`): the counterpart of
    /// [`Self::go_back`]. `false` when there is nothing ahead.
    pub fn go_forward(&mut self) -> bool {
        if self.hpos + 1 >= self.history.len() {
            return false;
        }
        self.hpos += 1;
        self.visiting = Some(self.history[self.hpos]);
        self.detail_scroll = 0;
        true
    }

    /// Emacs `help-goto-next-page`: scroll the displayed help text down one
    /// screenful.
    pub fn goto_next_page(&mut self) {
        self.detail_scroll = self.detail_scroll.saturating_add(self.page.max(1));
    }

    /// Emacs `help-goto-previous-page`: scroll it up one screenful.
    pub fn goto_previous_page(&mut self) {
        self.detail_scroll = self.detail_scroll.saturating_sub(self.page.max(1));
    }

    /// The title of the entry currently shown on its own, if any — so a command
    /// can report what it moved to.
    pub fn current_title(&self) -> Option<&str> {
        self.visiting.map(|i| self.entries[i].title.as_str())
    }

    /// Construct the browser pre-filtered to `filter` — used by `:Helptags` to
    /// land on the fuzzy-picked entry.
    pub fn with_filter(filter: String) -> Self {
        let mut p = Self::new();
        p.filter = filter;
        p
    }

    /// Every entry title (static commands, `:typables`, and topics) — the source
    /// list for the `:Helptags` fzf picker.
    pub fn entry_titles(&self) -> Vec<String> {
        self.entries.iter().map(|e| e.title.clone()).collect()
    }

    fn matches(&self) -> Vec<usize> {
        // While a single entry is being visited (Emacs's one-topic *Help*), the
        // view is exactly that entry — that is what go-back/go-forward move.
        if let Some(i) = self.visiting {
            return vec![i];
        }
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
        // `C-c` armed the Emacs `C-c C-b` / `C-c C-f` chords: the next key either
        // completes one or drops the prefix, as an Emacs prefix key does.
        if std::mem::take(&mut self.pending_ctrl_c) {
            match key {
                ctrl!('b') => {
                    self.go_back();
                }
                ctrl!('f') => {
                    self.go_forward();
                }
                _ => {}
            }
            return EventResult::Consumed(None);
        }
        match key {
            key!(Esc) => {
                return EventResult::Consumed(Some(Box::new(|c: &mut Compositor, _| {
                    c.pop();
                })))
            }
            ctrl!('c') => self.pending_ctrl_c = true,
            key!(Tab) | shift!(Tab) => {
                let i = CATS.iter().position(|(c, _)| *c == self.cat).unwrap_or(0);
                let step = if key == shift!(Tab) {
                    CATS.len() - 1
                } else {
                    1
                };
                self.cat = CATS[(i + step) % CATS.len()].0;
                self.sel = 0;
                self.top = 0;
            }
            key!(Down) | ctrl!('n') | ctrl!('j') => {
                if n > 0 {
                    self.sel = (self.sel + 1).min(n - 1);
                    self.detail_scroll = 0;
                }
            }
            key!(Up) | ctrl!('p') | ctrl!('k') => {
                self.sel = self.sel.saturating_sub(1);
                self.detail_scroll = 0;
            }
            key!(PageDown) => self.goto_next_page(),
            key!(PageUp) => self.goto_previous_page(),
            key!(Enter) => {
                // `help-follow`: follow the cross-reference at point — visit the
                // selected entry, showing it on its own and recording it in the
                // history that help-go-back / help-go-forward walk.
                if let Some(&e) = self.matches().get(self.sel) {
                    self.visit(e);
                    self.sel = 0;
                }
            }
            key!(Backspace) => {
                self.visiting = None;
                self.filter.pop();
                self.sel = 0;
            }
            // Help-mode keys, live while a single topic is displayed on its own —
            // that state is the read-only `*Help*` buffer. While browsing, these
            // letters are search input (the fall-through arm below).
            key!('l') if self.visiting.is_some() => {
                self.go_back();
            }
            key!('r') if self.visiting.is_some() => {
                self.go_forward();
            }
            key!('n') if self.visiting.is_some() => self.goto_next_page(),
            key!('p') if self.visiting.is_some() => self.goto_previous_page(),
            _ => {
                if let KeyCode::Char(c) = key.code {
                    self.visiting = None;
                    self.filter.push(c);
                    self.sel = 0;
                }
            }
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
        // `transparent-background`: drop the page fill so the terminal shows through.
        let mut page_bg = theme.get("ui.background");
        if ctx.editor.config().transparent_background {
            page_bg.bg = None;
        }
        surface.clear_with(area, page_bg);

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
        // Remember the detail height so help-goto-next-page scrolls a screenful.
        self.page = body_h.saturating_sub(1).max(1);
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
            Paragraph::new(Span::styled(
                if self.visiting.is_some() {
                    " l / C-c C-b back · r / C-c C-f forward · n / p page · ⏎ visit · ⌫ back to search · Esc close"
                } else {
                    " type to search · ↑/↓ or C-n/C-p/C-j/C-k move · ⏎ visit · Tab category · PgUp/PgDn scroll doc · Esc close"
                },
                dim,
            )),
            Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1),
            surface,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_history_walks_back_and_forward() {
        let mut p = HelpPanel::new();
        let first = p.entries[0].title.clone();
        let second = p.entries[1].title.clone();

        p.visit(0);
        p.visit(1);
        assert_eq!(p.current_title(), Some(second.as_str()));
        // Visiting shows exactly that entry, like Emacs's one-topic *Help*.
        assert_eq!(p.matches(), vec![1]);

        assert!(p.go_back());
        assert_eq!(p.current_title(), Some(first.as_str()));
        assert!(!p.go_back(), "nothing before the first visit");

        assert!(p.go_forward());
        assert_eq!(p.current_title(), Some(second.as_str()));
        assert!(!p.go_forward(), "nothing after the last visit");

        // A new visit from the middle truncates the forward history.
        p.go_back();
        p.visit(2);
        assert!(!p.go_forward());
        assert!(p.go_back());
        assert_eq!(p.current_title(), Some(first.as_str()));
    }

    #[test]
    fn help_paging_moves_by_a_screenful() {
        let mut p = HelpPanel::new();
        p.page = 20;
        p.goto_next_page();
        p.goto_next_page();
        assert_eq!(p.detail_scroll, 40);
        p.goto_previous_page();
        assert_eq!(p.detail_scroll, 20);
        p.goto_previous_page();
        p.goto_previous_page();
        assert_eq!(p.detail_scroll, 0, "scroll saturates at the top");
    }

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
