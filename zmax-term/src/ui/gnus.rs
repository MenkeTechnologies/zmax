//! Gnus — the zmax port of the GNU Emacs newsreader's group and summary buffers.
//!
//! A full-screen [`Component`] over [`crate::gnus`], which owns the NNTP
//! protocol, the local mbox spool backend and the `.newsrc` group statuses. The
//! reader has the two panes the Emacs manual describes:
//!
//! **Group buffer** (`gnus-group-mode`)
//!
//!   SPC     open the summary buffer for the group on this line
//!           (`gnus-group-read-group`)
//!   l / A s list subscribed groups that have unread articles
//!           (`gnus-group-list-groups`, the default listing)
//!   L / A u list all subscribed and unsubscribed groups
//!           (`gnus-group-list-all-groups`)
//!   A k     list killed groups   (`gnus-group-list-killed`)
//!   A z     list zombie groups   (`gnus-group-list-zombies`)
//!   u       toggle the group's subscription; a killed or zombie group becomes
//!           unsubscribed (`gnus-group-toggle-subscription-at-point`)
//!   C-k     kill the group on this line (`gnus-group-kill-group`)
//!   n       next unread group    (`gnus-group-next-unread-group`)
//!   p / DEL previous unread group (`gnus-group-prev-unread-group`)
//!   C-n/C-p next / previous line, unread or not
//!   q       save `.newsrc` and quit Gnus (`gnus-group-exit`)
//!
//! **Summary buffer** (`gnus-summary-mode`)
//!
//!   SPC     select the article on this line, or scroll the selected article a
//!           page; at its end select the next unread article
//!           (`gnus-summary-next-page`)
//!   DEL     scroll the article backwards (`gnus-summary-prev-page`)
//!   n / p   select the next / previous unread article
//!           (`gnus-summary-next-unread-article` / `-prev-unread-article`)
//!   C-n/C-p move over the summary lines without selecting
//!   s       incremental search inside the selected article
//!           (`gnus-summary-isearch-article`)
//!   M-s M-s REGEXP RET  search forward for an article matching REGEXP
//!           (`gnus-summary-search-article-forward`)
//!   M-s M-r / M-r REGEXP RET  the same, backwards
//!           (`gnus-summary-search-article-backward`)
//!   q       back to the group buffer (`gnus-summary-exit`)

use tui::buffer::Buffer as Surface;
use zmax_view::graphics::Rect;

use crate::gnus::{self, Group, Level, Listing, Overview, Server};
use crate::{
    alt,
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// Which pane is up.
#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Group,
    Summary,
}

/// What an active inline prompt will do with its input.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PromptAction {
    /// `s` — incremental search in the article pane; scrolls as you type.
    IsearchArticle,
    /// `M-s M-s` — regexp search over the following articles.
    SearchForward,
    /// `M-s M-r` / `M-r` — regexp search over the preceding articles.
    SearchBackward,
}

/// An inline minibuffer owned by the component (a pushed layer could not mutate
/// the reader's own state on submit).
struct Prompt {
    label: &'static str,
    input: String,
    action: PromptAction,
    /// Article scroll to restore when an incremental search is aborted.
    saved_scroll: usize,
}

/// The interactive newsreader overlay.
pub struct Gnus {
    server: Server,
    /// Every group known from `.newsrc`, the killed/zombie sidecar and the
    /// server's `LIST ACTIVE`, in that merge order.
    groups: Vec<Group>,
    listing: Listing,
    /// Indices into `groups` that the current listing shows.
    shown: Vec<usize>,
    /// Cursor within `shown`.
    cursor: usize,
    view: View,

    /// The group the summary buffer is showing, as an index into `groups`.
    group_idx: usize,
    arts: Vec<Overview>,
    /// Cursor within `arts`.
    art_cursor: usize,
    /// The article number currently displayed, if one is selected.
    selected: Option<u64>,
    /// Rendered lines of the selected article.
    article: Vec<String>,
    /// Article scroll offset, in rendered lines.
    scroll: usize,
    /// Rows the article pane last drew, so SPC can page by a real screenful.
    page_rows: usize,

    /// `true` after `A`, awaiting the second key of the group buffer's `A` map.
    pending_a: bool,
    /// `true` after `M-s`, awaiting `M-s` or `M-r`.
    pending_meta_s: bool,
    prompt: Option<Prompt>,
    status: String,
}

impl Gnus {
    /// Open the reader on `spec` (see [`Server::open`]): connect, read
    /// `.newsrc` and the killed/zombie sidecar, then merge in the server's group
    /// list. Blocking — the `gnus` command runs it on a blocking task.
    pub fn open(spec: &str) -> Result<Gnus, String> {
        let mut server = Server::open(spec).map_err(|e| format!("gnus: {e}"))?;
        let newsrc = gnus::read_optional(&gnus::newsrc_path());
        // No `.newsrc` at all means this is the first session, which is what
        // decides whether unknown groups are killed or zombified.
        let first_run = newsrc.is_none();
        let mut groups = gnus::parse_newsrc(newsrc.as_deref().unwrap_or(""));
        groups.extend(gnus::parse_sidecar(
            &gnus::read_optional(&gnus::sidecar_path()).unwrap_or_default(),
        ));
        let active = server
            .list_active()
            .map_err(|e| format!("gnus: {}: {e}", server.describe()))?;
        gnus::merge_active(&mut groups, &active, first_run);
        Ok(Gnus::with_groups(server, groups))
    }

    /// Build a reader over an already-resolved group list, showing the default
    /// (`l`) listing. [`Gnus::open`] calls this once it has merged `.newsrc`,
    /// the sidecar and the server's `LIST ACTIVE`.
    fn with_groups(server: Server, groups: Vec<Group>) -> Gnus {
        let mut reader = Gnus {
            server,
            groups,
            listing: Listing::Unread,
            shown: Vec::new(),
            cursor: 0,
            view: View::Group,
            group_idx: 0,
            arts: Vec::new(),
            art_cursor: 0,
            selected: None,
            article: Vec::new(),
            scroll: 0,
            page_rows: 10,
            pending_a: false,
            pending_meta_s: false,
            prompt: None,
            status: String::new(),
        };
        reader.relist();
        reader
    }

    // --- group buffer -------------------------------------------------------

    /// Recompute `shown` from `listing`, keeping the cursor in range.
    fn relist(&mut self) {
        self.shown = (0..self.groups.len())
            .filter(|&i| self.groups[i].in_listing(self.listing))
            .collect();
        self.cursor = self.cursor.min(self.shown.len().saturating_sub(1));
    }

    /// Switch the listing (`l`, `L`, `A k`, `A z`) and report what it holds.
    pub fn list(&mut self, listing: Listing) {
        self.listing = listing;
        self.cursor = 0;
        self.relist();
        self.status = format!("{} {} groups", self.shown.len(), listing.label());
    }

    /// The `groups` index under the group-buffer cursor.
    fn current_group(&self) -> Option<usize> {
        self.shown.get(self.cursor).copied()
    }

    /// `gnus-group-next-unread-group` / `-prev-unread-group`: move to the next
    /// or previous line whose group still has unread articles. With no such line
    /// left, the cursor stays put and the mode line says so.
    pub fn move_unread(&mut self, forward: bool) {
        let len = self.shown.len();
        if len == 0 {
            return;
        }
        let mut i = self.cursor;
        loop {
            let next = if forward {
                if i + 1 >= len {
                    self.status = "No more unread newsgroups".to_string();
                    return;
                }
                i + 1
            } else {
                if i == 0 {
                    self.status = "No previous unread newsgroup".to_string();
                    return;
                }
                i - 1
            };
            i = next;
            if self.groups[self.shown[i]].unread() > 0 {
                self.cursor = i;
                return;
            }
        }
    }

    /// Plain line motion (`C-n` / `C-p`), unread or not.
    fn move_line(&mut self, forward: bool) {
        if self.shown.is_empty() {
            return;
        }
        if forward {
            self.cursor = (self.cursor + 1).min(self.shown.len() - 1);
        } else {
            self.cursor = self.cursor.saturating_sub(1);
        }
    }

    /// `gnus-group-toggle-subscription-at-point` (`u`): subscribed ⇄
    /// unsubscribed; a killed or zombie group becomes unsubscribed.
    pub fn toggle_subscription(&mut self) {
        let Some(idx) = self.current_group() else {
            return;
        };
        let group = &mut self.groups[idx];
        group.level = match group.level {
            Level::Subscribed => Level::Unsubscribed,
            // The manual: "Invoking this on a killed or zombie group turns it
            // into an unsubscribed group."
            Level::Unsubscribed | Level::Killed | Level::Zombie => Level::Subscribed,
        };
        let (name, level) = (group.name.clone(), group.level);
        self.status = format!(
            "{name} is now {}",
            match level {
                Level::Subscribed => "subscribed",
                _ => "unsubscribed",
            }
        );
        self.relist();
    }

    /// `gnus-group-kill-group` (`C-k`): kill the group on this line. Killed
    /// groups leave `.newsrc` and drop out of the `l` and `L` listings.
    pub fn kill_group(&mut self) {
        let Some(idx) = self.current_group() else {
            return;
        };
        self.groups[idx].level = Level::Killed;
        self.status = format!("Killed group {}", self.groups[idx].name);
        self.relist();
    }

    /// `gnus-group-read-group` (SPC): fetch the group's overviews and switch to
    /// the summary buffer.
    pub fn read_group(&mut self) {
        let Some(idx) = self.current_group() else {
            self.status = "No group on this line".to_string();
            return;
        };
        let (name, low, high) = {
            let g = &self.groups[idx];
            (g.name.clone(), g.low, g.high)
        };
        match self.server.overviews(&name, low, high) {
            Ok(arts) => {
                self.group_idx = idx;
                self.arts = arts;
                self.art_cursor = self
                    .arts
                    .iter()
                    .position(|a| !self.groups[idx].is_read(a.number))
                    .unwrap_or(0);
                self.selected = None;
                self.article.clear();
                self.scroll = 0;
                self.view = View::Summary;
                self.status = format!("{}: {} articles", name, self.arts.len());
            }
            Err(e) => self.status = format!("{name}: {e}"),
        }
    }

    /// `gnus-group-exit` (`q`): write `.newsrc` and the killed/zombie sidecar,
    /// then close the reader. Returns the error text when a write failed.
    pub fn save(&self) -> Result<(), String> {
        let newsrc = gnus::format_newsrc(&self.groups);
        std::fs::write(gnus::newsrc_path(), newsrc)
            .map_err(|e| format!("gnus: {}: {e}", gnus::newsrc_path().display()))?;
        let sidecar = gnus::format_sidecar(&self.groups);
        std::fs::write(gnus::sidecar_path(), sidecar)
            .map_err(|e| format!("gnus: {}: {e}", gnus::sidecar_path().display()))?;
        Ok(())
    }

    // --- summary buffer -----------------------------------------------------

    /// Fetch and display article `number`, marking it read.
    fn select_article(&mut self, number: u64) {
        let Some(name) = self.groups.get(self.group_idx).map(|g| g.name.clone()) else {
            return;
        };
        match self.server.article(&name, number) {
            Ok(text) => {
                self.article = render_article(&text);
                self.selected = Some(number);
                self.scroll = 0;
                self.groups[self.group_idx].mark_read(number);
                if let Some(i) = self.arts.iter().position(|a| a.number == number) {
                    self.art_cursor = i;
                }
                self.status.clear();
            }
            Err(e) => self.status = format!("article {number}: {e}"),
        }
    }

    /// `gnus-summary-next-page` (SPC). Three documented behaviours in one key:
    /// select the article on this line when none is selected, otherwise scroll
    /// the article a page, and on reaching its end select the next unread
    /// article.
    pub fn next_page(&mut self) {
        if self.selected.is_none() {
            match self.arts.get(self.art_cursor) {
                Some(a) => {
                    let n = a.number;
                    self.select_article(n);
                }
                None => self.status = "No articles in this group".to_string(),
            }
            return;
        }
        let max_scroll = self.article.len().saturating_sub(self.page_rows);
        if self.scroll < max_scroll {
            self.scroll = (self.scroll + self.page_rows).min(max_scroll);
            return;
        }
        self.next_unread_article();
    }

    /// `gnus-summary-prev-page` (DEL): scroll the article backwards.
    pub fn prev_page(&mut self) {
        if self.selected.is_none() {
            self.status = "No article selected".to_string();
            return;
        }
        self.scroll = self.scroll.saturating_sub(self.page_rows);
    }

    /// `gnus-summary-next-unread-article` (`n`) and its backwards twin.
    fn move_unread_article(&mut self, forward: bool) {
        let group = self.group_idx;
        let start = self.art_cursor;
        let found = if forward {
            (start + 1..self.arts.len()).find(|&i| !self.groups[group].is_read(self.arts[i].number))
        } else {
            (0..start)
                .rev()
                .find(|&i| !self.groups[group].is_read(self.arts[i].number))
        };
        match found {
            Some(i) => {
                let n = self.arts[i].number;
                self.select_article(n);
            }
            None => {
                self.status = if forward {
                    "No more unread articles".to_string()
                } else {
                    "No previous unread article".to_string()
                }
            }
        }
    }

    /// `gnus-summary-next-unread-article` (`n`).
    pub fn next_unread_article(&mut self) {
        self.move_unread_article(true);
    }

    /// `gnus-summary-prev-unread-article` (`p`).
    pub fn prev_unread_article(&mut self) {
        self.move_unread_article(false);
    }

    /// Plain summary-line motion (`C-n` / `C-p`) without selecting.
    fn move_art_line(&mut self, forward: bool) {
        if self.arts.is_empty() {
            return;
        }
        if forward {
            self.art_cursor = (self.art_cursor + 1).min(self.arts.len() - 1);
        } else {
            self.art_cursor = self.art_cursor.saturating_sub(1);
        }
    }

    /// `gnus-summary-exit` (`q`): back to the group buffer, with the read marks
    /// this session set carried into the group listing.
    pub fn summary_exit(&mut self) {
        self.view = View::Group;
        self.selected = None;
        self.article.clear();
        self.arts.clear();
        self.relist();
        // Keep the cursor on the group just left when the listing still shows it.
        if let Some(pos) = self.shown.iter().position(|&i| i == self.group_idx) {
            self.cursor = pos;
        }
    }

    /// Open an inline prompt.
    fn ask(&mut self, label: &'static str, action: PromptAction) {
        self.prompt = Some(Prompt {
            label,
            input: String::new(),
            action,
            saved_scroll: self.scroll,
        });
    }

    /// `gnus-summary-isearch-article` (`s`).
    pub fn isearch_article(&mut self) {
        if self.selected.is_none() {
            self.status = "No article selected (SPC selects one)".to_string();
            return;
        }
        self.ask("I-search: ", PromptAction::IsearchArticle);
    }

    /// `gnus-summary-search-article-forward` (`M-s M-s`).
    pub fn search_article_forward(&mut self) {
        self.ask("Search forward (regexp): ", PromptAction::SearchForward);
    }

    /// `gnus-summary-search-article-backward` (`M-s M-r`, `M-r`).
    pub fn search_article_backward(&mut self) {
        self.ask("Search backward (regexp): ", PromptAction::SearchBackward);
    }

    /// Move the article pane to the first line at or after the current scroll
    /// that contains `needle` — the incremental part of `s`.
    fn isearch_step(&mut self, needle: &str) {
        if needle.is_empty() {
            return;
        }
        let lower = needle.to_lowercase();
        let hit = self
            .article
            .iter()
            .position(|l| l.to_lowercase().contains(&lower));
        match hit {
            Some(i) => {
                self.scroll = i;
                self.status.clear();
            }
            None => self.status = format!("Failing I-search: {needle}"),
        }
    }

    /// `gnus-summary-search-article-{forward,backward}`: walk the articles from
    /// the cursor looking for one whose text matches `pattern`, and select it.
    fn search_articles(&mut self, pattern: &str, forward: bool) {
        // The summary commands are reachable from the group buffer too, where
        // there is no group to search.
        let Some(name) = self.groups.get(self.group_idx).map(|g| g.name.clone()) else {
            self.status = "No summary buffer (SPC reads a group)".to_string();
            return;
        };
        let re = match regex::RegexBuilder::new(pattern)
            .case_insensitive(true)
            .build()
        {
            Ok(re) => re,
            Err(e) => {
                self.status = format!("bad regexp: {e}");
                return;
            }
        };
        let order: Vec<usize> = if forward {
            (self.art_cursor + 1..self.arts.len()).collect()
        } else {
            (0..self.art_cursor).rev().collect()
        };
        for i in order {
            let number = self.arts[i].number;
            // The overview line is free; only fall back to the full article when
            // the headers alone do not match.
            let head = format!("{} {}", self.arts[i].subject, self.arts[i].from);
            let hit = re.is_match(&head)
                || self
                    .server
                    .article(&name, number)
                    .map(|t| re.is_match(&t))
                    .unwrap_or(false);
            if hit {
                self.select_article(number);
                self.status = format!("Found {pattern} in article {number}");
                return;
            }
        }
        self.status = format!("No article matching {pattern}");
    }

    /// Feed one key to the active prompt.
    fn prompt_key(&mut self, key: zmax_view::input::KeyEvent) {
        let Some(prompt) = self.prompt.as_mut() else {
            return;
        };
        match key {
            key!(Esc) => {
                let restore = prompt.saved_scroll;
                let incremental = prompt.action == PromptAction::IsearchArticle;
                self.prompt = None;
                if incremental {
                    self.scroll = restore;
                }
                self.status.clear();
            }
            key!(Enter) => {
                let Some(prompt) = self.prompt.take() else {
                    return;
                };
                match prompt.action {
                    PromptAction::IsearchArticle => {}
                    PromptAction::SearchForward => self.search_articles(&prompt.input, true),
                    PromptAction::SearchBackward => self.search_articles(&prompt.input, false),
                }
            }
            key!(Backspace) => {
                prompt.input.pop();
                if prompt.action == PromptAction::IsearchArticle {
                    let needle = prompt.input.clone();
                    self.isearch_step(&needle);
                }
            }
            // A typed character. Written as the KeyEvent itself rather than
            // `key!(..)` so a shifted capital reaches the prompt too.
            zmax_view::input::KeyEvent {
                code: zmax_view::keyboard::KeyCode::Char(c),
                ..
            } => {
                prompt.input.push(c);
                if prompt.action == PromptAction::IsearchArticle {
                    let needle = prompt.input.clone();
                    self.isearch_step(&needle);
                }
            }
            _ => {}
        }
    }

    // --- rendering helpers ---------------------------------------------------

    /// One group-buffer line: `<mark> <unread> <name>`, the Gnus column order.
    fn group_line(&self, idx: usize, width: usize) -> String {
        let g = &self.groups[idx];
        let line = format!("{} {:>6}  {}", g.level.mark(), g.unread(), g.name);
        truncate(&line, width)
    }

    /// One summary line: `<mark> <number> <author> <subject>`.
    fn summary_line(&self, i: usize, width: usize) -> String {
        let a = &self.arts[i];
        let mark = if self
            .groups
            .get(self.group_idx)
            .is_some_and(|g| g.is_read(a.number))
        {
            'R'
        } else {
            'O'
        };
        let author = truncate(a.author(), 18);
        let line = format!("{mark} {:>6}  {author:<18}  {}", a.number, a.subject);
        truncate(&line, width)
    }
}

/// Format an article for the article pane: a pruned header block, a blank line,
/// then the body.
fn render_article(text: &str) -> Vec<String> {
    const SHOWN: [&str; 5] = ["From", "Subject", "Newsgroups", "Date", "Message-ID"];
    let (headers, body) = gnus::split_article(text);
    let mut out: Vec<String> = Vec::new();
    for want in SHOWN {
        if let Some((k, v)) = headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(want)) {
            out.push(format!("{k}: {v}"));
        }
    }
    out.push(String::new());
    out.extend(body.lines().map(|l| l.to_string()));
    out
}

/// Clip `s` to `width` display columns, marking the cut with `…`.
fn truncate(s: &str, width: usize) -> String {
    if s.chars().count() <= width {
        return s.to_string();
    }
    let keep = width.saturating_sub(1);
    let mut out: String = s.chars().take(keep).collect();
    out.push('…');
    out
}

impl Component for Gnus {
    fn handle_event(&mut self, event: &Event, _cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };
        let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });

        // An active prompt swallows keys until RET/Esc.
        if self.prompt.is_some() {
            self.prompt_key(key);
            return EventResult::Consumed(None);
        }
        self.status.clear();

        // Second key of the group buffer's `A` map.
        if std::mem::take(&mut self.pending_a) {
            match key {
                key!('k') => self.list(Listing::Killed),
                key!('z') => self.list(Listing::Zombies),
                key!('s') => self.list(Listing::Unread),
                key!('u') => self.list(Listing::All),
                _ => {}
            }
            return EventResult::Consumed(None);
        }

        // Second key of the summary buffer's `M-s` map.
        if std::mem::take(&mut self.pending_meta_s) {
            match key {
                alt!('s') => self.search_article_forward(),
                alt!('r') => self.search_article_backward(),
                _ => {}
            }
            return EventResult::Consumed(None);
        }

        match self.view {
            View::Group => match key {
                key!(' ') => self.read_group(),
                key!('l') => self.list(Listing::Unread),
                key!('L') => self.list(Listing::All),
                key!('A') => self.pending_a = true,
                key!('u') => self.toggle_subscription(),
                ctrl!('k') => self.kill_group(),
                key!('n') => self.move_unread(true),
                key!('p') | key!(Backspace) | key!(Delete) => self.move_unread(false),
                ctrl!('n') => self.move_line(true),
                ctrl!('p') => self.move_line(false),
                key!('q') | key!(Esc) => {
                    if let Err(e) = self.save() {
                        self.status = e;
                        return EventResult::Consumed(None);
                    }
                    return EventResult::Consumed(Some(close));
                }
                _ => {}
            },
            View::Summary => match key {
                key!(' ') => self.next_page(),
                key!(Backspace) | key!(Delete) => self.prev_page(),
                key!('n') => self.next_unread_article(),
                key!('p') => self.prev_unread_article(),
                ctrl!('n') => self.move_art_line(true),
                ctrl!('p') => self.move_art_line(false),
                key!(Enter) => {
                    if let Some(a) = self.arts.get(self.art_cursor) {
                        let n = a.number;
                        self.select_article(n);
                    }
                }
                key!('s') => self.isearch_article(),
                // `M-s` is a prefix (`M-s M-s`, `M-s M-r`); `M-r` also works on
                // its own, as gnus-summary-mode binds it.
                alt!('s') => self.pending_meta_s = true,
                alt!('r') => self.search_article_backward(),
                key!('q') | key!(Esc) => self.summary_exit(),
                _ => {}
            },
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let mut bg = theme.get("ui.background");
        if ctx.editor.config().transparent_background {
            bg.bg = None;
        }
        let header_style = theme.get("ui.text.focus");
        let text_style = theme.get("ui.text");
        let info_style = theme.get("ui.linenr");
        let sel_style = theme.get("ui.selection");

        surface.clear_with(area, bg);
        if area.width < 20 || area.height < 5 {
            return;
        }
        let width = area.width as usize;

        match self.view {
            View::Group => {
                let mode = format!(
                    " *Group*  {}  {} {} groups",
                    self.server.describe(),
                    self.shown.len(),
                    self.listing.label()
                );
                surface.set_stringn(area.x, area.y, &mode, width, header_style);
                let hint = "SPC read  l/L list  A k/z  u sub  C-k kill  n/p unread  q quit";
                if mode.len() + hint.len() + 3 < width {
                    surface.set_stringn(
                        area.x + area.width - hint.len() as u16 - 1,
                        area.y,
                        hint,
                        hint.len(),
                        info_style,
                    );
                }
                let rows = area.height.saturating_sub(3) as usize;
                let top = self.cursor.saturating_sub(rows.saturating_sub(1));
                for (row, &idx) in self.shown.iter().skip(top).take(rows).enumerate() {
                    let style = if top + row == self.cursor {
                        sel_style
                    } else {
                        text_style
                    };
                    surface.set_stringn(
                        area.x,
                        area.y + 2 + row as u16,
                        &self.group_line(idx, width),
                        width,
                        style,
                    );
                }
            }
            View::Summary => {
                let name = self
                    .groups
                    .get(self.group_idx)
                    .map(|g| g.name.as_str())
                    .unwrap_or("");
                let mode = format!(" *Summary {name}*  {} articles", self.arts.len());
                surface.set_stringn(area.x, area.y, &mode, width, header_style);
                let hint = "SPC page  DEL back  n/p unread  s isearch  M-s M-s search  q exit";
                if mode.len() + hint.len() + 3 < width {
                    surface.set_stringn(
                        area.x + area.width - hint.len() as u16 - 1,
                        area.y,
                        hint,
                        hint.len(),
                        info_style,
                    );
                }
                // With an article selected the pane splits, the way Gnus shows
                // the summary and article buffers in two windows.
                let body = area.height.saturating_sub(3);
                let sum_rows = if self.selected.is_some() {
                    (body / 2).max(1) as usize
                } else {
                    body as usize
                };
                let top = self.art_cursor.saturating_sub(sum_rows.saturating_sub(1));
                for row in 0..sum_rows.min(self.arts.len().saturating_sub(top)) {
                    let i = top + row;
                    let style = if i == self.art_cursor {
                        sel_style
                    } else {
                        text_style
                    };
                    surface.set_stringn(
                        area.x,
                        area.y + 2 + row as u16,
                        &self.summary_line(i, width),
                        width,
                        style,
                    );
                }
                if self.selected.is_some() {
                    let sep_y = area.y + 2 + sum_rows as u16;
                    surface.set_stringn(area.x, sep_y, &"─".repeat(width), width, info_style);
                    let art_rows = area.height.saturating_sub(sum_rows as u16 + 4).max(1) as usize;
                    self.page_rows = art_rows;
                    let scroll = self.scroll.min(self.article.len().saturating_sub(art_rows));
                    for (row, line) in self.article.iter().skip(scroll).take(art_rows).enumerate() {
                        surface.set_stringn(
                            area.x,
                            sep_y + 1 + row as u16,
                            line,
                            width,
                            text_style,
                        );
                    }
                }
            }
        }

        // Bottom row: the active prompt, else the status line.
        let bottom = area.y + area.height - 1;
        if let Some(prompt) = self.prompt.as_ref() {
            let line = format!("{}{}", prompt.label, prompt.input);
            surface.set_stringn(area.x, bottom, &line, width, info_style);
        } else if !self.status.is_empty() {
            surface.set_stringn(area.x, bottom, &self.status, width, info_style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn article_pane_shows_the_pruned_headers_then_the_body() {
        let lines = render_article(
            "Path: news!x\nFrom: a@b.c\nNewsgroups: comp.lang.rust\nSubject: Hi\n\nBody.\n",
        );
        // `Path:` is not in the shown set; the rest keep the documented order.
        assert_eq!(lines[0], "From: a@b.c");
        assert_eq!(lines[1], "Subject: Hi");
        assert_eq!(lines[2], "Newsgroups: comp.lang.rust");
        assert_eq!(lines[3], "");
        assert_eq!(lines[4], "Body.");
    }

    #[test]
    fn truncate_marks_the_cut() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("abcdefghij", 5), "abcd…");
    }

    /// Build a two-article mbox spool in a temp directory and return a reader on
    /// it, with the group subscribed and nothing read yet.
    fn spool_reader() -> (tempfile::TempDir, Gnus) {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(
            dir.path().join("comp.lang.rust"),
            "From a@b.c Mon Jan  1 00:00:00 2035\n\
             From: Ann Author <a@b.c>\n\
             Subject: borrow checker\n\n\
             The borrow checker rejected my code.\n\
             \n\
             From d@e.f Tue Jan  2 00:00:00 2035\n\
             From: Bob Builder <d@e.f>\n\
             Subject: lifetimes\n\n\
             Lifetimes are elided here.\n\n",
        )
        .expect("write spool");
        let mut server = Server::Local(dir.path().to_path_buf());
        let active = server.list_active().expect("list active");
        let mut groups = vec![Group::new("comp.lang.rust", Level::Subscribed)];
        crate::gnus::merge_active(&mut groups, &active, false);
        (dir, Gnus::with_groups(server, groups))
    }

    /// The documented SPC behaviour, in the three states the manual names: with
    /// nothing selected it selects the article on the line; at the end of that
    /// article it moves on to the next unread one; with none left it says so.
    #[test]
    fn spc_selects_then_pages_then_moves_to_the_next_unread_article() {
        let (_dir, mut g) = spool_reader();
        assert_eq!(g.shown.len(), 1, "the subscribed group has unread articles");
        g.read_group();
        assert_eq!(g.arts.len(), 2);
        assert!(
            g.selected.is_none(),
            "reading a group selects no article yet"
        );

        g.next_page();
        assert_eq!(g.selected, Some(1));
        assert!(g.article.iter().any(|l| l.contains("borrow checker")));
        assert!(g.groups[0].is_read(1), "a displayed article is marked read");

        // The article is shorter than a page, so the next SPC moves on.
        g.next_page();
        assert_eq!(g.selected, Some(2));
        assert!(g.article.iter().any(|l| l.contains("Lifetimes are elided")));

        g.next_page();
        assert_eq!(g.status, "No more unread articles");
        assert_eq!(g.groups[0].unread(), 0);

        // `q` in the summary buffer returns to the group buffer, and the group
        // has dropped out of the default (unread-only) listing.
        g.summary_exit();
        assert!(g.shown.is_empty());
    }

    /// `M-s M-s REGEXP` selects the next article whose text matches; the
    /// backwards search walks the other way.
    #[test]
    fn regexp_search_selects_the_matching_article() {
        let (_dir, mut g) = spool_reader();
        g.read_group();
        g.search_articles("elid(ed|ing)", true);
        assert_eq!(g.selected, Some(2));
        g.search_articles("rejected", false);
        assert_eq!(g.selected, Some(1));
        g.search_articles("no such text anywhere", true);
        assert_eq!(g.status, "No article matching no such text anywhere");
    }

    /// `u` and `C-k` move a group between the listings the manual describes.
    #[test]
    fn subscription_and_kill_change_which_listing_shows_the_group() {
        let (_dir, mut g) = spool_reader();
        g.toggle_subscription(); // subscribed -> unsubscribed
        assert_eq!(g.groups[0].level, Level::Unsubscribed);
        assert!(g.shown.is_empty(), "`l` shows subscribed groups only");
        g.list(Listing::All);
        assert_eq!(g.shown.len(), 1, "`L` shows unsubscribed groups too");

        g.kill_group();
        assert_eq!(g.groups[0].level, Level::Killed);
        assert!(g.shown.is_empty(), "`L` never shows killed groups");
        g.list(Listing::Killed);
        assert_eq!(g.shown.len(), 1, "`A k` shows them");

        // The manual: on a killed group `u` makes it unsubscribed.
        g.toggle_subscription();
        assert_eq!(g.groups[0].level, Level::Subscribed);
    }

    /// `s` scrolls the article pane to the first line carrying the search text,
    /// and Esc puts the scroll back.
    #[test]
    fn isearch_scrolls_the_article_and_esc_restores_it() {
        let (_dir, mut g) = spool_reader();
        g.read_group();
        g.next_page();
        g.isearch_article();
        for c in "rejected".chars() {
            g.prompt_key(zmax_view::input::KeyEvent {
                code: zmax_view::keyboard::KeyCode::Char(c),
                modifiers: zmax_view::keyboard::KeyModifiers::NONE,
            });
        }
        let hit = g.scroll;
        assert!(g.article[hit].contains("rejected"));
        g.prompt_key(key!(Esc));
        assert_eq!(g.scroll, 0);
        assert!(g.prompt.is_none());
    }
}
