//! The Services view (JetBrains Services tool window, `RunDashboard.*`): the
//! run configurations with whether each is running, finished or not started,
//! grouped by status, by type — the program the command runs — or by the
//! folders the user puts them in. Configurations can be hidden from the view
//! and restored, and a whole type removed from it.
//!
//! Keys: j/k move · Enter run · s stop · g group by status · t group by type
//! · f put in a folder · u take out of its folder · h hide · H restore all
//! hidden · r restore one · x remove the type · e edit · q quit.

use tui::buffer::Buffer as Surface;
use zmax_view::graphics::Rect;

use crate::compositor::{Callback, Component, Compositor, Context, Event, EventResult};
use crate::run_config::{self, RunConfig, RunConfigs};
use crate::{ctrl, key};

/// Where a configuration is in its life, as the view groups it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RunStatus {
    Running,
    Finished,
    NotStarted,
}

impl RunStatus {
    fn label(self) -> &'static str {
        match self {
            RunStatus::Running => "Running",
            RunStatus::Finished => "Finished",
            RunStatus::NotStarted => "Not Started",
        }
    }
}

/// A configuration's type: the program its command runs (`cargo`, `npm`).
pub fn config_type(config: &RunConfig) -> String {
    config
        .command
        .split_whitespace()
        .find(|w| !w.contains('='))
        .map(|w| w.rsplit('/').next().unwrap_or(w).to_string())
        .unwrap_or_else(|| "shell".to_string())
}

#[derive(Clone)]
enum Row {
    Group { label: String, depth: usize },
    Config { index: usize, depth: usize, status: RunStatus },
}

pub struct ServicesView {
    data: RunConfigs,
    /// The commands the Run window has run, and whether each still runs.
    runs: Vec<(String, bool)>,
    by_status: bool,
    by_type: bool,
    rows: Vec<Row>,
    selected: usize,
    status: String,
}

impl ServicesView {
    pub fn new(runs: Vec<(String, bool)>) -> Self {
        let mut view = ServicesView {
            data: run_config::load(),
            runs,
            by_status: true,
            by_type: false,
            rows: Vec::new(),
            selected: 0,
            status: String::new(),
        };
        view.build_rows();
        view
    }

    fn status_of(&self, config: &RunConfig) -> RunStatus {
        let line = config.command_line();
        match self.runs.iter().rev().find(|(cmd, _)| *cmd == line || cmd.ends_with(&config.command)) {
            Some((_, true)) => RunStatus::Running,
            Some((_, false)) => RunStatus::Finished,
            None => RunStatus::NotStarted,
        }
    }

    /// The groups a configuration sits under, outermost first.
    fn groups(&self, config: &RunConfig) -> Vec<String> {
        let mut groups = Vec::new();
        if self.by_status {
            groups.push(self.status_of(config).label().to_string());
        }
        if self.by_type {
            groups.push(config_type(config));
        }
        if !config.folder.is_empty() {
            groups.push(format!("📁 {}", config.folder));
        }
        groups
    }

    fn build_rows(&mut self) {
        let mut order: Vec<(Vec<String>, usize)> = (0..self.data.configs.len())
            .filter(|&i| {
                let config = &self.data.configs[i];
                !config.hidden && !self.data.removed_types.contains(&config_type(config))
            })
            .map(|i| (self.groups(&self.data.configs[i]), i))
            .collect();
        order.sort_by(|a, b| a.0.cmp(&b.0));
        let mut rows = Vec::new();
        let mut open: Vec<String> = Vec::new();
        for (groups, index) in order {
            let common = open.iter().zip(&groups).take_while(|(a, b)| a == b).count();
            for (depth, label) in groups.iter().enumerate().skip(common) {
                rows.push(Row::Group { label: label.clone(), depth });
            }
            let status = self.status_of(&self.data.configs[index]);
            rows.push(Row::Config { index, depth: groups.len(), status });
            open = groups;
        }
        self.rows = rows;
        self.selected = self.selected.min(self.rows.len().saturating_sub(1));
    }

    fn selected_index(&self) -> Option<usize> {
        match self.rows.get(self.selected)? {
            Row::Config { index, .. } => Some(*index),
            Row::Group { .. } => None,
        }
    }

    fn change(&mut self, f: impl FnOnce(&mut RunConfigs)) {
        f(&mut self.data);
        run_config::save(&self.data);
        self.build_rows();
    }

    fn run_selected(&self) -> Option<Callback> {
        let config = self.data.configs.get(self.selected_index()?)?.clone();
        Some(Box::new(move |compositor: &mut Compositor, cx: &mut Context| {
            compositor.pop();
            if let Some(view) = compositor.find::<crate::ui::EditorView>() {
                view.start_run(cx, config.command_line(), run_config::resolve_dir(&config.dir));
            }
        }))
    }
}

/// A picker over hidden configurations; picking one shows it again.
fn restore_picker(data: &RunConfigs) -> Option<Callback> {
    let hidden: Vec<String> = data.configs.iter().filter(|c| c.hidden).map(|c| c.name.clone()).collect();
    if hidden.is_empty() {
        return None;
    }
    Some(Box::new(move |compositor: &mut Compositor, _| {
        let columns = [crate::ui::PickerColumn::new("hidden configuration", |n: &String, _: &()| n.as_str().into())];
        let picker = crate::ui::Picker::new(columns, 0, hidden, (), |cx, name: &String, _| {
            restore_config(name);
            cx.editor.set_status(format!("{name} shown again"));
            crate::compositor::defer([Box::new(|compositor: &mut Compositor, _: &mut Context| {
                if let Some(view) = compositor.find::<ServicesView>() {
                    view.data = run_config::load();
                    view.build_rows();
                }
            }) as Callback]);
        });
        compositor.push(Box::new(crate::ui::overlay::overlaid(picker)));
    }))
}

/// JetBrains "Restore Configuration": `name` is shown in the view again.
pub fn restore_config(name: &str) {
    let mut data = run_config::load();
    for config in data.configs.iter_mut().filter(|c| c.name == name) {
        config.hidden = false;
    }
    run_config::save(&data);
}

impl Component for ServicesView {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        let Event::Key(key) = event else {
            return EventResult::Ignored(None);
        };
        self.status.clear();
        let last = self.rows.len().saturating_sub(1);
        match *key {
            key!('q') | key!(Esc) | ctrl!('c') => {
                return EventResult::Consumed(Some(Box::new(|compositor: &mut Compositor, _| {
                    compositor.pop();
                })))
            }
            key!('j') | key!(Down) => self.selected = (self.selected + 1).min(last),
            key!('k') | key!(Up) => self.selected = self.selected.saturating_sub(1),
            key!(Enter) => return EventResult::Consumed(self.run_selected()),
            key!('s') => {
                return EventResult::Consumed(Some(Box::new(|compositor: &mut Compositor, cx: &mut Context| {
                    if let Some(view) = compositor.find::<crate::ui::EditorView>() {
                        view.with_ide(|ide| ide.stop_run());
                    }
                    cx.editor.set_status("run stopped");
                })))
            }
            key!('g') => {
                self.by_status = !self.by_status;
                self.build_rows();
            }
            key!('t') => {
                self.by_type = !self.by_type;
                self.build_rows();
            }
            key!('f') => {
                if let Some(index) = self.selected_index() {
                    let prompt = crate::ui::Prompt::new(
                        "folder: ".into(),
                        None,
                        crate::ui::completers::none,
                        move |cx: &mut Context, input: &str, event: crate::ui::PromptEvent| {
                            if event != crate::ui::PromptEvent::Validate || input.trim().is_empty() {
                                return;
                            }
                            let folder = input.trim().to_string();
                            let mut data = run_config::load();
                            if let Some(config) = data.configs.get_mut(index) {
                                config.folder = folder.clone();
                            }
                            run_config::save(&data);
                            cx.editor.set_status(format!("in folder {folder}"));
                            crate::compositor::defer([Box::new(|compositor: &mut Compositor, _: &mut Context| {
                                if let Some(view) = compositor.find::<ServicesView>() {
                                    view.data = run_config::load();
                                    view.build_rows();
                                }
                            }) as Callback]);
                        },
                    );
                    return EventResult::Consumed(Some(Box::new(move |compositor: &mut Compositor, _| {
                        compositor.push(Box::new(prompt));
                    })));
                }
            }
            key!('u') => {
                if let Some(index) = self.selected_index() {
                    self.change(|data| data.configs[index].folder.clear());
                }
            }
            key!('h') => {
                if let Some(index) = self.selected_index() {
                    self.change(|data| data.configs[index].hidden = true);
                    self.status = "configuration hidden — H restores all, r one".into();
                }
            }
            key!('H') => self.change(|data| data.configs.iter_mut().for_each(|c| c.hidden = false)),
            key!('r') => match restore_picker(&self.data) {
                Some(callback) => return EventResult::Consumed(Some(callback)),
                None => self.status = "no hidden configurations".into(),
            },
            key!('x') => {
                if let Some(index) = self.selected_index() {
                    let kind = config_type(&self.data.configs[index]);
                    self.status = format!("{kind} configurations removed from the view");
                    self.change(|data| data.removed_types.push(kind));
                }
            }
            key!('e') => {
                return EventResult::Consumed(Some(Box::new(|compositor: &mut Compositor, _| {
                    compositor.pop();
                    compositor.push(Box::new(crate::ui::preferences::PreferencesPanel::new(3)));
                })))
            }
            _ => {}
        }
        if !self.status.is_empty() {
            cx.editor.set_status(self.status.clone());
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, cx: &mut Context) {
        let theme = &cx.editor.theme;
        surface.clear_with(area, theme.get("ui.background"));
        if area.height < 3 {
            return;
        }
        let title = format!(
            " Services — {} configurations   by status {}  by type {}",
            self.data.configs.len(),
            if self.by_status { "on" } else { "off" },
            if self.by_type { "on" } else { "off" },
        );
        surface.set_stringn(area.x, area.y, &title, area.width as usize, theme.get("ui.text.focus"));
        let body_h = area.height.saturating_sub(2) as usize;
        let top = self.selected.saturating_sub(body_h.saturating_sub(1));
        for (i, row) in self.rows.iter().enumerate().skip(top).take(body_h) {
            let y = area.y + 1 + (i - top) as u16;
            let (line, style) = match row {
                Row::Group { label, depth } => (format!("{}▾ {label}", "  ".repeat(*depth)), theme.get("ui.text.directory")),
                Row::Config { index, depth, status } => {
                    let config = &self.data.configs[*index];
                    let mark = match status {
                        RunStatus::Running => "▶",
                        RunStatus::Finished => "■",
                        RunStatus::NotStarted => "·",
                    };
                    (format!("{}{mark} {}  {}", "  ".repeat(*depth), config.name, config.command), theme.get("ui.text"))
                }
            };
            let style = if i == self.selected { theme.get("ui.selection") } else { style };
            surface.set_stringn(area.x, y, &line, area.width as usize, style);
        }
        if self.rows.is_empty() {
            surface.set_stringn(area.x, area.y + 1, "  no run configurations shown — H restores hidden ones", area.width as usize, theme.get("comment"));
        }
        let footer = "⏎ run  s stop  g status  t type  f folder  u unfolder  h hide  H/r restore  x remove type  e edit  q";
        surface.set_stringn(area.x, area.y + area.height - 1, footer, area.width as usize, theme.get("ui.linenr"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_configuration_type_is_its_program() {
        let config = |command: &str| RunConfig { command: command.into(), ..Default::default() };
        assert_eq!("cargo", config_type(&config("cargo run --release")));
        assert_eq!("node", config_type(&config("NODE_ENV=dev /usr/bin/node app.js")));
        assert_eq!("shell", config_type(&config("")));
    }
}
