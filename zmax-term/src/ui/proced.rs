//! Proced — a process viewer/manager, the zmax port of GNU Emacs `proced`.
//!
//! A full-screen [`Component`] listing the system's processes (via `ps`), one
//! row each with a left-hand mark column and the pid/ppid/user/%cpu/%mem/comm
//! columns. Like Dired the current row is highlighted and a `HashSet` of marked
//! pids drives batch actions. All pure logic (parsing `ps`, sorting, filtering)
//! lives in the I/O-free, unit-tested [`zmax_core::proced`]; this module runs
//! `ps`/`kill`, renders and handles keys.
//!
//! Keys (parsed into a `proced` keymap mode by `scripts/gen_port_report.py`, so
//! each maps to its Emacs proced counterpart in the port tracker):
//!   n/j/Down/C-n, p/Up/C-p — move point (Emacs proced uses n/p; j is a vim-style
//!     extra. `k` is NOT up — it sends a signal, matching Emacs proced.)
//!   Home/End — first / last process
//!   m — mark (and advance)          (proced-mark)
//!   u — unmark (and advance)        (proced-unmark)
//!   M — mark all                    (proced-mark-all)
//!   s — cycle sort key              (proced-sort)
//!   P — sort by pid                 (proced-sort-pid)
//!   C — sort by %cpu                (proced-sort-pcpu)
//!   R — sort by %mem                (proced-sort-pmem)
//!   / — refine: type an incremental filter needle, Backspace edits, Enter keeps
//!       it, Esc clears + leaves refine mode   (proced-refine)
//!   g — update: re-run `ps`         (proced-revert / proced-update)
//!   k — send SIGTERM to the marked pids (or the pid at point) via `kill`
//!                                   (proced-send-signal)
//!   q/Esc/C-c — quit
//!
//! Deferred: proced-toggle-tree (parent/child indentation) — the substrate
//! captures ppid but tree rendering/collapsing is left to a later slice.

use std::collections::HashSet;
use std::process::Command;

use tui::buffer::Buffer as Surface;
use zmax_core::proced::{filter, parse_ps, sort_procs, Proc, Sort};
use zmax_view::graphics::Rect;
use zmax_view::keyboard::{KeyCode, KeyModifiers};

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// The interactive Proced overlay.
pub struct Proced {
    procs: Vec<Proc>,
    /// Marked pids, driving batch signal-sending (survive re-sort/refresh).
    marked: HashSet<u32>,
    /// Index into the currently *visible* (filtered) rows.
    selected: usize,
    scroll: usize,
    viewport: usize,
    sort: Sort,
    /// Incremental refine needle (matched on user/comm, case-insensitive).
    filter: String,
    /// True while typing into `filter` (`/`): printable keys extend the needle.
    refining: bool,
    status: Option<String>,
    error: Option<String>,
}

impl Proced {
    /// Open Proced, running `ps` immediately. On failure the list is empty and
    /// an error line is shown.
    pub fn new() -> Self {
        let mut p = Proced {
            procs: Vec::new(),
            marked: HashSet::new(),
            selected: 0,
            scroll: 0,
            viewport: 1,
            sort: Sort::Cpu,
            filter: String::new(),
            refining: false,
            status: None,
            error: None,
        };
        p.refresh();
        p
    }

    /// Re-run `ps` and reparse (Emacs `proced-update`). Marks for pids that have
    /// gone are dropped; the list is re-sorted by the active key.
    fn refresh(&mut self) {
        match Command::new("ps")
            .args(["-axo", "pid,ppid,user,pcpu,pmem,comm"])
            .output()
        {
            Ok(out) if out.status.success() => {
                let text = String::from_utf8_lossy(&out.stdout);
                self.procs = parse_ps(&text);
                sort_procs(&mut self.procs, self.sort);
                self.error = None;
            }
            Ok(out) => {
                self.procs.clear();
                let err = String::from_utf8_lossy(&out.stderr);
                self.error = Some(format!("ps failed: {}", err.trim()));
            }
            Err(e) => {
                self.procs.clear();
                self.error = Some(format!("ps: {e}"));
            }
        }
        let present: HashSet<u32> = self.procs.iter().map(|p| p.pid).collect();
        self.marked.retain(|pid| present.contains(pid));
        self.clamp();
    }

    /// The processes currently shown, honouring the refine needle.
    fn visible(&self) -> Vec<&Proc> {
        filter(&self.procs, &self.filter)
    }

    fn clamp(&mut self) {
        let n = self.visible().len();
        if self.selected >= n {
            self.selected = n.saturating_sub(1);
        }
    }

    fn move_selection(&mut self, delta: isize) {
        let n = self.visible().len();
        if n == 0 {
            return;
        }
        let max = n as isize - 1;
        self.selected = (self.selected as isize + delta).clamp(0, max) as usize;
    }

    fn current_pid(&self) -> Option<u32> {
        self.visible().get(self.selected).map(|p| p.pid)
    }

    /// The pids to act on: the marked set (in table order) if non-empty, else the
    /// pid at point.
    fn targets(&self) -> Vec<u32> {
        if !self.marked.is_empty() {
            self.procs
                .iter()
                .filter(|p| self.marked.contains(&p.pid))
                .map(|p| p.pid)
                .collect()
        } else {
            self.current_pid().into_iter().collect()
        }
    }

    fn set_sort(&mut self, by: Sort) {
        self.sort = by;
        sort_procs(&mut self.procs, self.sort);
        self.status = Some(format!("proced: sorted by {}", self.sort.label()));
    }

    /// Send SIGTERM to the target pids via `kill` (Emacs `proced-send-signal`).
    fn send_signal(&mut self) {
        let pids = self.targets();
        if pids.is_empty() {
            self.status = Some("proced: no process selected".into());
            return;
        }
        let mut killed = 0;
        for pid in &pids {
            let ok = Command::new("kill")
                .arg(pid.to_string())
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if ok {
                killed += 1;
            }
        }
        self.marked.clear();
        self.refresh();
        self.status = Some(format!(
            "proced: sent SIGTERM to {killed} of {} process(es)",
            pids.len()
        ));
    }

    /// Handle a key while the refine needle is being typed. Returns once the key
    /// is consumed by the mini-editor.
    fn refine_key(&mut self, key: zmax_view::input::KeyEvent) {
        match key {
            key!(Enter) => self.refining = false,
            key!(Esc) => {
                self.refining = false;
                self.filter.clear();
                self.clamp();
            }
            key!(Backspace) => {
                self.filter.pop();
                self.selected = 0;
            }
            _ => {
                if let KeyCode::Char(c) = key.code {
                    if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT {
                        self.filter.push(c);
                        self.selected = 0;
                    }
                }
            }
        }
    }
}

impl Default for Proced {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Proced {
    fn handle_event(&mut self, event: &Event, _cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };
        if self.refining {
            self.refine_key(key);
            return EventResult::Consumed(None);
        }
        let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });
        match key {
            key!('q') | key!(Esc) | ctrl!('c') => return EventResult::Consumed(Some(close)),
            key!('j') | key!('n') | key!(Down) | ctrl!('n') => self.move_selection(1),
            key!('p') | key!(Up) | ctrl!('p') => self.move_selection(-1),
            key!(Home) => self.selected = 0,
            key!(End) => self.selected = self.visible().len().saturating_sub(1),
            key!('m') => {
                if let Some(pid) = self.current_pid() {
                    self.marked.insert(pid);
                    self.move_selection(1);
                }
            }
            key!('u') => {
                if let Some(pid) = self.current_pid() {
                    self.marked.remove(&pid);
                    self.move_selection(1);
                }
            }
            key!('M') => {
                for p in &self.procs {
                    self.marked.insert(p.pid);
                }
            }
            key!('s') => self.set_sort(self.sort.next()),
            key!('P') => self.set_sort(Sort::Pid),
            key!('C') => self.set_sort(Sort::Cpu),
            key!('R') => self.set_sort(Sort::Mem),
            key!('/') => {
                self.refining = true;
                self.status = None;
            }
            key!('g') => {
                self.refresh();
                self.status = Some("proced: updated".into());
            }
            key!('k') => self.send_signal(),
            _ => {}
        }
        // Stay modal: never leak keys to the editor behind us.
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let mut bg = theme.get("ui.background");
        // `transparent-background`: drop the panel fill so the terminal shows
        // through, matching the editor surface and the rest of the IDE.
        if ctx.editor.config().transparent_background {
            bg.bg = None;
        }
        let header_style = theme.get("ui.text.focus");
        let col_style = theme.get("function");
        let text_style = theme.get("ui.text");
        let info_style = theme.get("ui.linenr");
        let sel_style = theme.get("ui.selection");
        let mark_style = theme.get("diff.plus");
        let error_style = theme.get("error");
        let warn_style = theme.get("warning");

        surface.clear_with(area, bg);
        if area.width < 12 || area.height < 4 {
            return;
        }

        let visible_len = self.visible().len();
        let title = format!(
            "Proced  {} processes  sort:{}{}",
            visible_len,
            self.sort.label(),
            if self.marked.is_empty() {
                String::new()
            } else {
                format!("  {} marked", self.marked.len())
            }
        );
        surface.set_stringn(area.x, area.y, &title, area.width as usize, header_style);

        // Status / error / refine-prompt line.
        if self.refining {
            let prompt = format!("refine (/): {}", self.filter);
            surface.set_stringn(area.x, area.y + 1, &prompt, area.width as usize, warn_style);
        } else if let Some(err) = &self.error {
            surface.set_stringn(area.x, area.y + 1, err, area.width as usize, error_style);
        } else if let Some(status) = &self.status {
            surface.set_stringn(area.x, area.y + 1, status, area.width as usize, warn_style);
        } else if !self.filter.is_empty() {
            let f = format!("filter: {}", self.filter);
            surface.set_stringn(area.x, area.y + 1, &f, area.width as usize, info_style);
        }

        // Column header.
        let header = format!(
            "{} {:>7} {:>7} {:<10} {:>5} {:>5}  {}",
            " ", "PID", "PPID", "USER", "%CPU", "%MEM", "COMMAND"
        );
        surface.set_stringn(area.x, area.y + 2, &header, area.width as usize, col_style);

        let body_y = area.y + 3;
        let body_h = area.height.saturating_sub(4);
        self.viewport = body_h.max(1) as usize;

        if visible_len == 0 {
            let msg = if self.error.is_some() {
                "(no processes)"
            } else {
                "(none)"
            };
            surface.set_stringn(area.x, body_y, msg, area.width as usize, info_style);
        } else {
            // Keep the selection in view.
            if self.selected < self.scroll {
                self.scroll = self.selected;
            } else if self.selected >= self.scroll + self.viewport {
                self.scroll = self.selected + 1 - self.viewport;
            }

            // Re-fetch the visible rows *after* the mutations above so the borrow
            // does not conflict with them.
            let view = self.visible();
            for (offset, p) in view
                .iter()
                .enumerate()
                .skip(self.scroll)
                .take(body_h as usize)
            {
                let y = body_y + (offset - self.scroll) as u16;
                let marked = self.marked.contains(&p.pid);
                let mark = if marked { '*' } else { ' ' };
                let user: String = p.user.chars().take(10).collect();
                let line = format!(
                    "{} {:>7} {:>7} {:<10} {:>5.1} {:>5.1}  {}",
                    mark, p.pid, p.ppid, user, p.cpu, p.mem, p.comm
                );
                let base = if offset == self.selected {
                    sel_style
                } else if marked {
                    mark_style
                } else {
                    text_style
                };
                surface.set_stringn(area.x, y, &line, area.width as usize, base);
                if marked {
                    surface.set_stringn(area.x, y, "*", 1, mark_style);
                }
            }
        }

        // Footer keys.
        let footer =
            "n/p move  m mark  u unmark  M all  s sort  P/C/R pid/cpu/mem  / refine  g update  k kill  q quit";
        surface.set_stringn(
            area.x,
            area.y + area.height - 1,
            footer,
            area.width as usize,
            info_style,
        );
    }
}
