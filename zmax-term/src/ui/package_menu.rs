//! The Package Menu — the zmax port of GNU Emacs `package-menu-mode`
//! (`M-x list-packages`).
//!
//! What a *package* is here is set out in [`zmax_core::package`]: a directory of
//! Emacs Lisp under `~/.zmax/elpa/<name>-<version>/`, fetched from a real ELPA
//! archive, put on the elisp load path and evaluated by the embedded `elisprs`
//! interpreter. This module is the listing: a full-screen modal Component over the
//! menu model, with Emacs's `package-menu-mode-map` on its real keys.
//!
//! The model lives in a process-global (`MENU`) rather than in the Component, so
//! that the `package-menu-*` commands in `commands.rs` — which are what `M-x`
//! reaches — act on the same rows the Component displays, whether or not the
//! Component happens to be open. The Component renders the global every frame, so
//! a mark made by `M-x package-menu-mark-install` shows up in the listing, and a
//! mark made with `i` in the listing is what `M-x package-menu-execute` installs.

use std::cmp::Ordering;
use std::sync::{Mutex, OnceLock};

use tui::buffer::Buffer as Surface;
use zmax_core::package::{Filter, Mark, PackageMenu, Status};
use zmax_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// The loaded package listing. Empty until `package-refresh-contents` /
/// `list-packages` fills it.
fn menu() -> &'static Mutex<PackageMenu> {
    static MENU: OnceLock<Mutex<PackageMenu>> = OnceLock::new();
    MENU.get_or_init(|| Mutex::new(PackageMenu::default()))
}

/// Run `f` against the package listing.
pub fn with_menu<T>(f: impl FnOnce(&mut PackageMenu) -> T) -> T {
    let mut guard = menu().lock().unwrap_or_else(|e| e.into_inner());
    f(&mut guard)
}

/// Replace the listing (what a refresh produces).
pub fn set_menu(new: PackageMenu) {
    let mut guard = menu().lock().unwrap_or_else(|e| e.into_inner());
    *guard = new;
}

/// Whether any archive listing has been loaded yet.
pub fn is_loaded() -> bool {
    with_menu(|m| !m.rows().is_empty())
}

/// The full-screen package listing.
#[derive(Default)]
pub struct PackageMenuView {
    /// First visible row (the model holds the selection; this is only scroll).
    scroll: usize,
    viewport: usize,
    /// The in-panel message under the listing (Emacs's echo area equivalent).
    status: String,
    /// `/` was pressed: the next key chooses which filter to apply.
    pending_filter: bool,
    /// `S` was pressed: the next key names the column to sort by.
    pending_sort: bool,
    /// Emacs's `tabulated-list-sort-key`: the column the listing is sorted by,
    /// and whether the order is flipped. `None` is the model's own order.
    sort: Option<(SortColumn, bool)>,
    /// A filter is being typed: which one, and the text so far.
    reading: Option<(FilterKind, String)>,
}

/// A sortable column of `package-menu-mode`'s `tabulated-list-format`
/// (package.el:3229). Spacemacs reaches them with `S P` and `S S`; the `Stars`
/// column and its `S *` binding come from paradox (paradox.el's
/// `paradox--column-index-star` / `paradox--star-predicate`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum SortColumn {
    Name,
    Status,
    /// paradox's GitHub-star column. zmax carries no star count on a `Row`
    /// (there is no field for it in [`zmax_core::package::Row`], and no network
    /// resolver), so every package ranks equal here and the sort falls through
    /// to the name predicate — which is what paradox itself does before its
    /// async `paradox--refresh-star-count` has filled the counts in.
    Stars,
}

impl SortColumn {
    fn label(self) -> &'static str {
        match self {
            SortColumn::Name => "package name",
            SortColumn::Status => "status",
            SortColumn::Stars => "GitHub stars",
        }
    }
}

/// The status order `package-menu--status-predicate` (package.el:4093) sorts by
/// — not alphabetical: available comes first, then installed, external, obsolete.
/// The statuses zmax has no model for (new, dependency, held, built-in, …) drop
/// out of the ranking, keeping the relative order of the ones it does.
fn status_rank(status: Status) -> u8 {
    match status {
        Status::Available => 0,
        Status::Installed => 1,
        Status::External => 2,
        Status::Obsolete => 3,
    }
}

/// Which `package-menu-filter-by-*` command the typed text will feed.
#[derive(Clone, Copy)]
enum FilterKind {
    Name,
    Description,
    NameOrDescription,
    Keyword,
    Status,
    Archive,
    Version,
}

impl FilterKind {
    fn prompt(self) -> &'static str {
        match self {
            FilterKind::Name => "Filter by name (regexp): ",
            FilterKind::Description => "Filter by description (regexp): ",
            FilterKind::NameOrDescription => "Filter by name or description: ",
            FilterKind::Keyword => "Filter by keyword: ",
            FilterKind::Status => "Filter by status: ",
            FilterKind::Archive => "Filter by archive: ",
            FilterKind::Version => "Filter by version: ",
        }
    }

    fn filter(self, text: String) -> Filter {
        match self {
            FilterKind::Name => Filter::Name(text),
            FilterKind::Description => Filter::Description(text),
            FilterKind::NameOrDescription => Filter::NameOrDescription(text),
            FilterKind::Keyword => Filter::Keyword(text),
            FilterKind::Status => Filter::Status(text),
            FilterKind::Archive => Filter::Archive(text),
            FilterKind::Version => Filter::Version(text),
        }
    }
}

/// The quick help `?` shows — Emacs `package-menu-quick-help`.
pub const QUICK_HELP: &str = "\
Package Menu — key bindings

  RET / v   describe the package under point
  i         mark for installation      d       mark for deletion
  u / DEL   unmark                     U       mark all upgradable packages
  ~         mark obsolete versions for deletion
  x         execute the marked installs and deletions
  g / r     refresh the archive listing
  w         browse the package's home page
  H         hide this package          (       show hidden packages again

  /n  filter by name            /d  filter by description
  /N  filter by name or description
  /k  filter by keyword         /s  filter by status
  /a  filter by archive         /v  filter by version
  /m  show only marked          /u  show only upgradable
  //  clear all filters

  SP  sort by package name      SS  sort by status
  S*  sort by GitHub stars       (the same column again reverses the order)

  q         quit the package menu
";

impl PackageMenuView {
    pub fn new() -> Self {
        Self {
            // `package-menu-mode` opens with `tabulated-list-sort-key` set to
            // `("Status")` (package.el:3235), so the listing starts in status
            // order and the first `S S` reverses it.
            sort: Some((SortColumn::Status, false)),
            ..Self::default()
        }
    }

    /// Feed one key to the filter being typed. Returns false when the read ended.
    fn filter_key(&mut self, key: zmax_view::input::KeyEvent) -> bool {
        use zmax_view::keyboard::{KeyCode, KeyModifiers};
        let Some((kind, text)) = self.reading.as_mut() else {
            return false;
        };
        match key {
            key!(Enter) => {
                let (kind, text) = (*kind, std::mem::take(text));
                self.reading = None;
                if text.is_empty() {
                    self.status = "package-menu: filter cancelled".into();
                } else {
                    let filter = kind.filter(text);
                    let label = filter.label();
                    with_menu(|m| m.add_filter(filter));
                    self.status = format!("package-menu: filtered by {label}");
                }
            }
            key!(Esc) | ctrl!('g') => {
                self.reading = None;
                self.status = "package-menu: filter cancelled".into();
            }
            key!(Backspace) => {
                text.pop();
            }
            zmax_view::input::KeyEvent {
                code: KeyCode::Char(c),
                modifiers,
            } if modifiers == KeyModifiers::NONE || modifiers == KeyModifiers::SHIFT => {
                text.push(c);
            }
            _ => {}
        }
        true
    }

    /// `/` then a key: which filter to read (Emacs's `package-menu-filter-*` map).
    fn dispatch_filter_key(&mut self, key: zmax_view::input::KeyEvent) {
        let kind = match key {
            key!('n') => Some(FilterKind::Name),
            key!('d') => Some(FilterKind::Description),
            key!('N') => Some(FilterKind::NameOrDescription),
            key!('k') => Some(FilterKind::Keyword),
            key!('s') => Some(FilterKind::Status),
            key!('a') => Some(FilterKind::Archive),
            key!('v') => Some(FilterKind::Version),
            // `/ m` — only the marked packages; `/ u` — only the upgradable ones.
            // Neither reads any text, so they apply immediately.
            key!('m') => {
                with_menu(|m| m.add_filter(Filter::Marked));
                self.status = "package-menu: showing marked packages".into();
                None
            }
            key!('u') => {
                with_menu(|m| m.add_filter(Filter::Upgradable));
                self.status = "package-menu: showing upgradable packages".into();
                None
            }
            key!('/') => {
                with_menu(PackageMenu::clear_filters);
                self.status = "package-menu: filters cleared".into();
                None
            }
            _ => None,
        };
        if let Some(kind) = kind {
            self.reading = Some((kind, String::new()));
        }
    }

    /// `S` then a key: the column to sort by. Emacs's `tabulated-list-sort`
    /// flips the order when the column already sorted is asked for again
    /// (tabulated-list.el:756).
    fn dispatch_sort_key(&mut self, key: zmax_view::input::KeyEvent) {
        let column = match key {
            key!('P') => SortColumn::Name,
            key!('S') => SortColumn::Status,
            // paradox `S *` — sort by GitHub stars.
            key!('*') => SortColumn::Stars,
            _ => return,
        };
        let reverse = match self.sort {
            Some((active, reverse)) if active == column => !reverse,
            _ => false,
        };
        self.sort = Some((column, reverse));
        self.status = format!(
            "package-menu: sorted by {}{}",
            column.label(),
            if reverse { " (descending)" } else { "" }
        );
    }

    /// The visible rows in display order, as indices into `PackageMenu::visible()`.
    /// Emacs re-prints the buffer in sorted order; here the model keeps its own
    /// order and the view permutes it, so `selected` always names a row the model
    /// can act on.
    fn order(&self, menu: &PackageMenu) -> Vec<usize> {
        let visible = menu.visible();
        let mut order: Vec<usize> = (0..visible.len()).collect();
        let Some((column, reverse)) = self.sort else {
            return order;
        };
        order.sort_by(|&a, &b| {
            let (a, b) = (visible[a], visible[b]);
            let by_column = match column {
                SortColumn::Name => Ordering::Equal,
                SortColumn::Status => status_rank(a.status).cmp(&status_rank(b.status)),
                // No per-row star count exists to compare, so every package ties
                // and the name tie-break below decides the order (see the
                // `SortColumn::Stars` doc comment).
                SortColumn::Stars => Ordering::Equal,
            };
            // Every column predicate falls back to `package-menu--name-predicate`
            // on a tie (package.el:4136).
            by_column.then_with(|| a.desc.name.cmp(&b.desc.name))
        });
        if reverse {
            // Package names are unique, so the comparison above is a total order
            // and reversing it is what the negated predicate would have produced.
            order.reverse();
        }
        order
    }

    /// Move the selection `delta` rows in *display* order — the same motion the
    /// model does when nothing is sorted.
    fn move_display(&self, menu: &mut PackageMenu, delta: isize) {
        if self.sort.is_none() {
            menu.move_selection(delta);
            return;
        }
        let order = self.order(menu);
        let Some(pos) = order.iter().position(|&i| i == menu.selected) else {
            menu.move_selection(delta);
            return;
        };
        let next = (pos as isize + delta).clamp(0, order.len() as isize - 1) as usize;
        menu.selected = order[next];
    }

    /// The first (`last` false) or last row of the listing as displayed.
    fn goto_display(&self, menu: &mut PackageMenu, last: bool) {
        let order = self.order(menu);
        menu.selected = if last {
            order.last().copied().unwrap_or(0)
        } else {
            order.first().copied().unwrap_or(0)
        };
    }

    /// Run one of the model's marking commands and redo the step it takes
    /// afterwards in display order.
    fn marking(
        &self,
        step: isize,
        f: impl FnOnce(&mut PackageMenu) -> Option<String>,
    ) -> Option<String> {
        with_menu(|menu| {
            let previous = menu.selected;
            let name = f(menu)?;
            menu.selected = previous;
            self.move_display(menu, step);
            Some(name)
        })
    }
}

impl Component for PackageMenuView {
    fn handle_event(&mut self, event: &Event, _cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };

        // A filter is being typed: every key belongs to it.
        if self.reading.is_some() {
            self.filter_key(key);
            return EventResult::Consumed(None);
        }
        // `/` armed the filter map: this key names the filter.
        if std::mem::take(&mut self.pending_filter) {
            self.dispatch_filter_key(key);
            return EventResult::Consumed(None);
        }
        // `S` armed the sort map: this key names the column.
        if std::mem::take(&mut self.pending_sort) {
            self.status.clear();
            self.dispatch_sort_key(key);
            return EventResult::Consumed(None);
        }

        self.status.clear();
        let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });

        match key {
            key!('q') | key!(Esc) | ctrl!('c') => return EventResult::Consumed(Some(close)),

            // Motion. Every motion is in display order, which is the model's own
            // order until an `S` sort permutes it.
            key!('j') | key!(Down) | key!('n') | ctrl!('n') => {
                with_menu(|m| self.move_display(m, 1))
            }
            key!('k') | key!(Up) | key!('p') | ctrl!('p') => {
                with_menu(|m| self.move_display(m, -1))
            }
            key!(PageDown) => with_menu(|m| self.move_display(m, 10)),
            key!(PageUp) => with_menu(|m| self.move_display(m, -10)),
            key!(Home) => with_menu(|m| self.goto_display(m, false)),
            key!(End) | key!('G') => with_menu(|m| self.goto_display(m, true)),

            // `i` / `d` / `~` / `u` / `DEL` / `U` — the marking commands.
            key!('i') => {
                if let Some(name) = self.marking(1, |m| m.mark_current(Mark::Install)) {
                    self.status = format!("package-menu: {name} marked for installation");
                }
            }
            key!('d') => {
                if let Some(name) = self.marking(1, |m| m.mark_current(Mark::Delete)) {
                    self.status = format!("package-menu: {name} marked for deletion");
                }
            }
            key!('u') => {
                self.marking(1, |m| m.unmark_current(1));
            }
            key!(Backspace) => {
                self.marking(-1, |m| m.unmark_current(-1));
            }
            key!('U') => {
                let n = with_menu(PackageMenu::mark_upgrades);
                self.status = format!("package-menu: {n} package(s) marked for upgrade");
            }
            key!('~') => {
                let installed = crate::commands::installed_packages();
                let n = with_menu(|m| m.mark_obsolete_for_deletion(&installed));
                self.status = format!("package-menu: {n} obsolete package(s) marked for deletion");
            }

            // `x` — do the marked work. The install/delete engine lives with the
            // commands, so this is the same code path `M-x package-menu-execute`
            // takes.
            key!('x') => {
                return EventResult::Consumed(Some(Box::new(
                    |_compositor: &mut Compositor, cx: &mut Context| {
                        crate::commands::package_menu_execute_impl(cx);
                    },
                )));
            }

            // `g` / `r` — refresh the archive listing.
            key!('g') | key!('r') => {
                return EventResult::Consumed(Some(Box::new(
                    |_compositor: &mut Compositor, cx: &mut Context| {
                        crate::commands::package_refresh_impl(cx);
                    },
                )));
            }

            // `RET` / `v` — describe. `w` — browse the home page.
            key!(Enter) | key!('v') => {
                return EventResult::Consumed(Some(Box::new(
                    |compositor: &mut Compositor, cx: &mut Context| {
                        compositor.pop();
                        crate::commands::describe_current_package(cx);
                    },
                )));
            }
            key!('w') => {
                return EventResult::Consumed(Some(Box::new(
                    |_compositor: &mut Compositor, cx: &mut Context| {
                        crate::commands::browse_current_package_url(cx);
                    },
                )));
            }

            // `H` — hide; `(` — show hidden again.
            key!('H') => {
                if let Some(name) = with_menu(PackageMenu::hide_current) {
                    self.status = format!("package-menu: {name} hidden");
                }
            }
            key!('(') => {
                let hiding = with_menu(PackageMenu::toggle_hiding);
                self.status = if hiding {
                    "package-menu: hiding hidden packages".into()
                } else {
                    "package-menu: showing hidden packages".into()
                };
            }

            // `/` — the filter map; `S` — the sort map.
            key!('/') => self.pending_filter = true,
            key!('S') => self.pending_sort = true,

            // `?` / `h` — quick help.
            key!('?') | key!('h') => {
                return EventResult::Consumed(Some(Box::new(
                    |compositor: &mut Compositor, cx: &mut Context| {
                        compositor.pop();
                        crate::commands::show_package_quick_help(cx.editor);
                    },
                )));
            }
            _ => {}
        }
        // Modal: no key reaches the buffer behind the listing.
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
        let mark_style = theme.get("diff.plus");
        let flag_style = theme.get("error");

        surface.clear_with(area, bg);
        if area.width < 24 || area.height < 4 {
            return;
        }
        let width = area.width as usize;

        // Header.
        let filters = with_menu(|m| {
            m.filters
                .iter()
                .map(Filter::label)
                .collect::<Vec<_>>()
                .join(" ")
        });
        let title = if filters.is_empty() {
            " Package Menu".to_string()
        } else {
            format!(" Package Menu [{filters}]")
        };
        // The sorted column carries the arrow `tabulated-list-init-header` puts
        // in the header line.
        let arrow = |column: SortColumn| match self.sort {
            Some((active, reverse)) if active == column => {
                if reverse {
                    "▼"
                } else {
                    "▲"
                }
            }
            _ => "",
        };
        let header = format!(
            "{:<38}{:<12}{:<10}{}",
            format!("{title}{}", arrow(SortColumn::Name)),
            "Version",
            format!("Status{}", arrow(SortColumn::Status)),
            "Summary"
        );
        surface.set_stringn(area.x, area.y, &header, width, header_style);
        let hint = "i install  d delete  x execute  U upgrades  / filter  S sort  ? help  q quit";
        surface.set_stringn(area.x, area.y + 1, hint, width, info_style);

        let body_y = area.y + 2;
        let body_h = area.height.saturating_sub(3) as usize;
        self.viewport = body_h;

        let (rows, selected, total) = with_menu(|m| {
            // The listing is drawn in display order; the model's `selected` names
            // a package, so its row keeps the point across a sort, as
            // `tabulated-list-print` does when it remembers the entry id.
            let order = self.order(m);
            let visible = m.visible();
            let selected = order.iter().position(|&i| i == m.selected).unwrap_or(0);
            let rows: Vec<(String, Option<Mark>, Status, bool)> = order
                .iter()
                .map(|&i| visible[i])
                .map(|r| {
                    let installed = r
                        .installed
                        .as_ref()
                        .map(|v| zmax_core::package::version_string(v))
                        .unwrap_or_default();
                    // Show the version on offer, and what is installed when it
                    // differs — which is the whole point of the `obsolete` status.
                    let version = if r.status == Status::Obsolete {
                        format!("{}<{}", installed, r.desc.version_string())
                    } else if r.status == Status::External {
                        installed
                    } else {
                        r.desc.version_string()
                    };
                    let line = format!(
                        "{:<36.36} {:<11.11} {:<9.9} {}",
                        r.desc.name,
                        version,
                        r.status.label(),
                        r.desc.summary
                    );
                    (line, r.mark, r.status, r.hidden)
                })
                .collect();
            let total = rows.len();
            (rows, selected, total)
        });

        // Keep the selection in view.
        if selected < self.scroll {
            self.scroll = selected;
        } else if body_h > 0 && selected >= self.scroll + body_h {
            self.scroll = selected + 1 - body_h;
        }
        if self.scroll > total.saturating_sub(1) {
            self.scroll = total.saturating_sub(1);
        }

        if total == 0 {
            let empty = if is_loaded() {
                "  (no package matches the current filter — `/ /` clears it)"
            } else {
                "  (no archive listing loaded — `g` fetches one)"
            };
            surface.set_stringn(area.x, body_y, empty, width, info_style);
        }

        for (offset, (line, mark, status, hidden)) in
            rows.iter().enumerate().skip(self.scroll).take(body_h)
        {
            let y = body_y + (offset - self.scroll) as u16;
            let base = if offset == selected {
                sel_style
            } else if *hidden {
                info_style
            } else {
                match (mark, status) {
                    (Some(Mark::Install), _) => mark_style,
                    (Some(Mark::Delete), _) => flag_style,
                    (None, Status::Obsolete) => header_style,
                    _ => text_style,
                }
            };
            let flag = mark.map_or(' ', Mark::char);
            surface.set_stringn(area.x, y, &format!("{flag} {line}"), width, base);
        }

        let footer = if let Some((kind, text)) = &self.reading {
            format!("{}{text}", kind.prompt())
        } else if self.pending_filter {
            "/ — n name  d description  N name-or-description  k keyword  s status  a archive  v version  m marked  u upgradable  / clear".to_string()
        } else if self.pending_sort {
            "S — P package name  S status  * GitHub stars  (again on the same column reverses)".to_string()
        } else if !self.status.is_empty() {
            self.status.clone()
        } else {
            let marked = with_menu(|m| m.marked().len());
            format!(
                "{total} package(s){}",
                if marked > 0 {
                    format!(", {marked} marked — `x` executes")
                } else {
                    String::new()
                }
            )
        };
        surface.set_stringn(
            area.x,
            area.y + area.height - 1,
            &footer,
            width,
            header_style,
        );
    }

    fn id(&self) -> Option<&'static str> {
        Some("package-menu")
    }
}
