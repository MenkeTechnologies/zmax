//! The build-tool window (JetBrains Gradle / Maven / Cargo tool window): the
//! projects of the workspace's build files as a tree, each with its tasks
//! and workspace members (`crate::build_tool`).
//!
//! Keys: j/k move · Enter run the task, or open/close the project ·
//! h/l close/open · E/C expand/collapse all · m group modules · t group
//! tasks · i inherited tasks · s show ignored · I ignore · d detach ·
//! x import or skip a member · o open the build file · r/R sync the
//! project/all · a trigger the task · T the triggers · b run the task before
//! the active run configuration · S auto-sync · q quit.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use tui::buffer::Buffer as Surface;
use zmax_view::graphics::Rect;

use crate::build_tool::{self, task_group, AutoSync, BuildProject, BuildState};
use crate::compositor::{Callback, Component, Compositor, Context, Event, EventResult};
use crate::{ctrl, key};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Row {
    Project {
        file: PathBuf,
        depth: usize,
        ignored: bool,
        member: bool,
        imported: bool,
    },
    Group {
        label: String,
        depth: usize,
    },
    Task {
        dir: PathBuf,
        command: String,
        depth: usize,
    },
}

pub struct BuildToolPanel {
    root: PathBuf,
    projects: Vec<BuildProject>,
    state: BuildState,
    expanded: HashSet<PathBuf>,
    rows: Vec<Row>,
    selected: usize,
    scroll: usize,
    status: String,
}

impl BuildToolPanel {
    pub fn new(root: PathBuf) -> Self {
        let projects = build_tool::discover(&root);
        let expanded = projects.iter().map(|p| p.file.clone()).collect();
        let mut panel = BuildToolPanel {
            root,
            projects,
            state: build_tool::load(),
            expanded,
            rows: Vec::new(),
            selected: 0,
            scroll: 0,
            status: String::new(),
        };
        panel.build_rows();
        panel
    }

    fn tasks(&self, rows: &mut Vec<Row>, dir: &Path, tasks: &[String], depth: usize) {
        if self.state.group_tasks {
            let mut groups: Vec<&str> = tasks.iter().map(|t| task_group(t)).collect();
            groups.sort_unstable();
            groups.dedup();
            for group in groups {
                rows.push(Row::Group {
                    label: group.to_string(),
                    depth,
                });
                for task in tasks.iter().filter(|t| task_group(t) == group) {
                    rows.push(Row::Task {
                        dir: dir.to_path_buf(),
                        command: task.clone(),
                        depth: depth + 1,
                    });
                }
            }
        } else {
            rows.extend(tasks.iter().map(|t| Row::Task {
                dir: dir.to_path_buf(),
                command: t.clone(),
                depth,
            }));
        }
    }

    fn project_rows(
        &self,
        rows: &mut Vec<Row>,
        project: &BuildProject,
        depth: usize,
        root_file: Option<&Path>,
    ) {
        let ignored = self.state.ignored.contains(&project.file);
        if self.state.detached.contains(&project.file) || (ignored && !self.state.show_ignored) {
            return;
        }
        let imported = root_file.is_none_or(|root| {
            self.state
                .imported
                .iter()
                .find(|(f, _)| f == root)
                .is_none_or(|(_, members)| members.contains(&project.file))
        });
        rows.push(Row::Project {
            file: project.file.clone(),
            depth,
            ignored,
            member: root_file.is_some(),
            imported,
        });
        if !self.expanded.contains(&project.file) {
            return;
        }
        self.tasks(rows, project.dir(), &project.tasks, depth + 1);
        // "Show Inherited Tasks": the members' tasks under their parent too.
        if self.state.show_inherited && root_file.is_none() {
            for member in &project.members {
                let name = member
                    .dir()
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                rows.push(Row::Group {
                    label: format!("{name} (inherited)"),
                    depth: depth + 1,
                });
                self.tasks(rows, member.dir(), &member.tasks, depth + 2);
            }
        }
        if self.state.group_modules {
            for member in &project.members {
                self.project_rows(rows, member, depth + 1, Some(&project.file));
            }
        }
    }

    fn build_rows(&mut self) {
        let mut rows = Vec::new();
        for project in &self.projects {
            self.project_rows(&mut rows, project, 0, None);
            // Without "Group Modules", members are listed beside their root.
            if !self.state.group_modules {
                for member in &project.members {
                    self.project_rows(&mut rows, member, 0, Some(&project.file));
                }
            }
        }
        self.rows = rows;
        self.selected = self.selected.min(self.rows.len().saturating_sub(1));
    }

    fn save(&mut self) {
        build_tool::save(&self.state);
        self.build_rows();
    }

    fn selected_project(&self) -> Option<PathBuf> {
        match self.rows.get(self.selected)? {
            Row::Project { file, .. } => Some(file.clone()),
            _ => self.rows[..self.selected]
                .iter()
                .rev()
                .find_map(|r| match r {
                    Row::Project { file, .. } => Some(file.clone()),
                    _ => None,
                }),
        }
    }

    fn selected_task(&self) -> Option<(PathBuf, String)> {
        match self.rows.get(self.selected)? {
            Row::Task { dir, command, .. } => Some((dir.clone(), command.clone())),
            _ => None,
        }
    }

    fn toggle(list: &mut Vec<PathBuf>, file: PathBuf) -> bool {
        match list.iter().position(|f| *f == file) {
            Some(i) => {
                list.remove(i);
                false
            }
            None => {
                list.push(file);
                true
            }
        }
    }

    /// JetBrains "Select Project Data to Import": a member is imported or not.
    fn toggle_imported(&mut self) {
        let Some(Row::Project {
            file, member: true, ..
        }) = self.rows.get(self.selected).cloned()
        else {
            self.status = "not a workspace member".into();
            return;
        };
        let Some(root) = self
            .projects
            .iter()
            .find(|p| p.members.iter().any(|m| m.file == file))
        else {
            return;
        };
        let all: Vec<PathBuf> = root.members.iter().map(|m| m.file.clone()).collect();
        let root_file = root.file.clone();
        let entry = match self
            .state
            .imported
            .iter_mut()
            .find(|(f, _)| *f == root_file)
        {
            Some(entry) => entry,
            None => {
                self.state.imported.push((root_file, all));
                self.state.imported.last_mut().expect("just pushed")
            }
        };
        let imported = Self::toggle(&mut entry.1, file);
        self.status = if imported {
            "member imported"
        } else {
            "member not imported"
        }
        .into();
        self.save();
    }

    fn run_task(dir: PathBuf, command: String) -> Callback {
        Box::new(move |compositor: &mut Compositor, cx: &mut Context| {
            if let Some(view) = compositor.find::<crate::ui::EditorView>() {
                view.start_run(cx, command, dir);
            }
        })
    }

    /// A picker of the moments a task can be triggered at.
    fn trigger_picker(dir: PathBuf, task: String) -> Callback {
        Box::new(move |compositor: &mut Compositor, _| {
            let columns = [crate::ui::PickerColumn::new(
                "run the task",
                |w: &build_tool::When, _: &()| w.label().into(),
            )];
            let picker = crate::ui::Picker::new(
                columns,
                0,
                build_tool::When::ALL,
                (),
                move |cx, when: &build_tool::When, _| {
                    let trigger = build_tool::Trigger {
                        when: *when,
                        dir: dir.clone(),
                        task: task.clone(),
                    };
                    build_tool::update(|state| {
                        if !state.triggers.contains(&trigger) {
                            state.triggers.push(trigger);
                        }
                    });
                    cx.editor
                        .set_status(format!("{} runs {}", task, when.label()));
                },
            );
            compositor.push(Box::new(crate::ui::overlay::overlaid(picker)));
        })
    }
}

/// JetBrains "Tasks Activation": the triggers, each removed by picking it.
pub fn triggers_manager() -> Option<Callback> {
    let triggers = build_tool::load().triggers;
    if triggers.is_empty() {
        return None;
    }
    Some(Box::new(move |compositor: &mut Compositor, _| {
        let columns = [
            crate::ui::PickerColumn::new("when", |t: &build_tool::Trigger, _: &()| {
                t.when.label().into()
            }),
            crate::ui::PickerColumn::new("task", |t: &build_tool::Trigger, _: &()| {
                t.task.as_str().into()
            }),
            crate::ui::PickerColumn::new("in", |t: &build_tool::Trigger, _: &()| {
                t.dir.display().to_string().into()
            }),
        ];
        let picker = crate::ui::Picker::new(
            columns,
            0,
            triggers,
            (),
            |cx, trigger: &build_tool::Trigger, _| {
                build_tool::update(|state| state.triggers.retain(|t| t != trigger));
                cx.editor.set_status(format!(
                    "removed: {} {}",
                    trigger.task,
                    trigger.when.label()
                ));
            },
        );
        compositor.push(Box::new(crate::ui::overlay::overlaid(picker)));
    }))
}

impl Component for BuildToolPanel {
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
            key!(Enter) => {
                if let Some((dir, command)) = self.selected_task() {
                    return EventResult::Consumed(Some(Self::run_task(dir, command)));
                }
                if let Some(Row::Project { file, .. }) = self.rows.get(self.selected).cloned() {
                    if !self.expanded.remove(&file) {
                        self.expanded.insert(file);
                    }
                    self.build_rows();
                }
            }
            key!('l') | key!(Right) => {
                if let Some(Row::Project { file, .. }) = self.rows.get(self.selected).cloned() {
                    self.expanded.insert(file);
                    self.build_rows();
                }
            }
            key!('h') | key!(Left) => {
                if let Some(file) = self.selected_project() {
                    self.expanded.remove(&file);
                    self.build_rows();
                    self.selected = self
                        .rows
                        .iter()
                        .position(|r| matches!(r, Row::Project { file: f, .. } if *f == file))
                        .unwrap_or(0);
                }
            }
            key!('E') => {
                for project in &self.projects {
                    self.expanded.insert(project.file.clone());
                    self.expanded
                        .extend(project.members.iter().map(|m| m.file.clone()));
                }
                self.build_rows();
            }
            key!('C') => {
                self.expanded.clear();
                self.build_rows();
            }
            key!('m') => {
                self.state.group_modules = !self.state.group_modules;
                self.save();
            }
            key!('t') => {
                self.state.group_tasks = !self.state.group_tasks;
                self.save();
            }
            key!('i') => {
                self.state.show_inherited = !self.state.show_inherited;
                self.save();
            }
            key!('s') => {
                self.state.show_ignored = !self.state.show_ignored;
                self.save();
            }
            key!('I') => {
                if let Some(file) = self.selected_project() {
                    let ignored = Self::toggle(&mut self.state.ignored, file);
                    self.status = if ignored {
                        "project ignored"
                    } else {
                        "project no longer ignored"
                    }
                    .into();
                    self.save();
                }
            }
            key!('d') => {
                if let Some(file) = self.selected_project() {
                    self.state.detached.push(file.clone());
                    self.status = format!("detached {}", file.display());
                    self.save();
                }
            }
            key!('x') => self.toggle_imported(),
            key!('o') => {
                if let Some(file) = self.selected_project() {
                    return EventResult::Consumed(Some(Box::new(
                        move |compositor: &mut Compositor, cx: &mut Context| {
                            compositor.pop();
                            if let Err(e) =
                                cx.editor.open(&file, zmax_view::editor::Action::Replace)
                            {
                                cx.editor.set_error(format!("{}: {e}", file.display()));
                            }
                        },
                    )));
                }
            }
            key!('r') | key!('R') => {
                let all = *key == key!('R');
                let scope = if all { None } else { self.selected_project() };
                return EventResult::Consumed(Some(Box::new(
                    move |compositor: &mut Compositor, cx: &mut Context| {
                        crate::commands::build_sync_now(compositor, cx, scope);
                    },
                )));
            }
            key!('a') => {
                if let Some((dir, task)) = self.selected_task() {
                    return EventResult::Consumed(Some(Self::trigger_picker(dir, task)));
                }
                self.status = "select a task".into();
            }
            key!('T') => match triggers_manager() {
                Some(callback) => return EventResult::Consumed(Some(callback)),
                None => self.status = "no task triggers".into(),
            },
            key!('b') => match self.selected_task() {
                Some((dir, task)) => {
                    self.status = match crate::run_config::add_before_task(&dir, &task) {
                        Some(name) => format!("{task} runs before {name}"),
                        None => "no active run configuration".into(),
                    };
                }
                None => self.status = "select a task".into(),
            },
            key!('S') => {
                self.state.auto_sync = match self.state.auto_sync {
                    AutoSync::Off => AutoSync::Any,
                    AutoSync::Any => AutoSync::Off,
                };
                self.status = format!("auto-sync: {:?}", self.state.auto_sync);
                self.save();
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
        let title =
            format!(
            " Build tools — {}   modules {}  tasks {}  inherited {}  ignored {}  auto-sync {:?}",
            self.root.display(),
            if self.state.group_modules { "grouped" } else { "flat" },
            if self.state.group_tasks { "grouped" } else { "flat" },
            if self.state.show_inherited { "shown" } else { "hidden" },
            if self.state.show_ignored { "shown" } else { "hidden" },
            self.state.auto_sync,
        );
        surface.set_stringn(
            area.x,
            area.y,
            &title,
            area.width as usize,
            theme.get("ui.text.focus"),
        );
        let body_h = area.height.saturating_sub(2) as usize;
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + body_h {
            self.scroll = self.selected + 1 - body_h;
        }
        if self.rows.is_empty() {
            surface.set_stringn(
                area.x,
                area.y + 1,
                "  no build files at the workspace root",
                area.width as usize,
                theme.get("comment"),
            );
        }
        for (i, row) in self.rows.iter().enumerate().skip(self.scroll).take(body_h) {
            let y = area.y + 1 + (i - self.scroll) as u16;
            let (line, style) = match row {
                Row::Project {
                    file,
                    depth,
                    ignored,
                    imported,
                    ..
                } => {
                    let open = if self.expanded.contains(file) {
                        "▾"
                    } else {
                        "▸"
                    };
                    let rel = file
                        .strip_prefix(&self.root)
                        .unwrap_or(file)
                        .display()
                        .to_string();
                    let mut line = format!("{}{open} {rel}", "  ".repeat(*depth));
                    if *ignored {
                        line.push_str("  (ignored)");
                    }
                    if !imported {
                        line.push_str("  (not imported)");
                    }
                    (line, theme.get("ui.text.directory"))
                }
                Row::Group { label, depth } => (
                    format!("{}▾ {label}", "  ".repeat(*depth)),
                    theme.get("comment"),
                ),
                Row::Task { command, depth, .. } => (
                    format!("{}⚙ {command}", "  ".repeat(*depth)),
                    theme.get("ui.text"),
                ),
            };
            let style = if i == self.selected {
                theme.get("ui.selection")
            } else {
                style
            };
            surface.set_stringn(area.x, y, &line, area.width as usize, style);
        }
        let footer = "⏎ run/open  h/l  E/C  m t i s groups  I ignore  d detach  x import  o open  r/R sync  a trigger  T triggers  b before run  S auto-sync  q";
        surface.set_stringn(
            area.x,
            area.y + area.height - 1,
            footer,
            area.width as usize,
            theme.get("ui.linenr"),
        );
    }
}
