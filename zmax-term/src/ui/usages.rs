//! The Usages view — JetBrains Find Usages' tool window.
//!
//! A full-screen modal [`Component`] over the usages of one symbol, as the
//! language server's references found them. Each usage is classified from its
//! line ([`zmax_core::usage_kind`]) and placed in groups the options choose:
//! usage type, test or production scope, module, directory (flat or as a
//! tree), file, and the enclosing class or function. Filters hide usages in
//! comments, imports and generated code, or keep only reads or only writes.
//! Usages can be excluded (kept, greyed) or removed from the list.
//!
//! The options are kept across views, as the IDE keeps them.
//!
//! Keys: n/p/j/k move · }/{ next/previous group · Enter/o go to the usage ·
//! x exclude or include · d remove · r rerun · q quit; and the options:
//! t usage type · s scope · m module · M flatten modules · D directory ·
//! T directory tree · f file structure · P short paths · c comments ·
//! i imports · G generated · R reads only · W writes only.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tui::buffer::Buffer as Surface;
use zmax_core::usage_kind::{self, UsageKind};
use zmax_core::Rope;
use zmax_view::graphics::Rect;
use zmax_view::Editor;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// Which usages the access filters keep.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    All,
    Read,
    Write,
}

/// The view's grouping and filtering options (JetBrains `UsageGrouping.*`
/// and `UsageFiltering.*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UsageOptions {
    pub by_type: bool,
    pub by_scope: bool,
    pub by_module: bool,
    pub flatten_modules: bool,
    pub by_directory: bool,
    pub directory_tree: bool,
    pub by_member: bool,
    pub short_paths: bool,
    pub show_comments: bool,
    pub show_imports: bool,
    pub show_generated: bool,
    pub access: Access,
}

static OPTIONS: Mutex<UsageOptions> = Mutex::new(UsageOptions {
    by_type: true,
    by_scope: false,
    by_module: false,
    flatten_modules: true,
    by_directory: false,
    directory_tree: false,
    by_member: false,
    short_paths: false,
    show_comments: true,
    show_imports: true,
    show_generated: true,
    access: Access::All,
});

pub fn options() -> UsageOptions {
    *OPTIONS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Change the options; returns them as they now are.
pub fn set_options(f: impl FnOnce(&mut UsageOptions)) -> UsageOptions {
    let mut options = OPTIONS.lock().unwrap_or_else(|e| e.into_inner());
    f(&mut options);
    *options
}

/// Where Find Usages looks (JetBrains "Find Usages Settings…" scopes).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UsageScope {
    Project,
    OpenFiles,
    File(PathBuf),
    Directory(PathBuf),
    Production,
    Test,
}

impl UsageScope {
    pub fn label(&self) -> String {
        match self {
            UsageScope::Project => "the project".into(),
            UsageScope::OpenFiles => "open files".into(),
            UsageScope::File(p) => format!("file {}", p.display()),
            UsageScope::Directory(p) => format!("directory {}", p.display()),
            UsageScope::Production => "production code".into(),
            UsageScope::Test => "test code".into(),
        }
    }

    pub fn contains(&self, editor: &Editor, path: &Path) -> bool {
        match self {
            UsageScope::Project => true,
            UsageScope::OpenFiles => editor.document_by_path(path).is_some(),
            UsageScope::File(p) => p == path,
            UsageScope::Directory(dir) => path.starts_with(dir),
            UsageScope::Production => !usage_kind::is_test_path(path),
            UsageScope::Test => usage_kind::is_test_path(path),
        }
    }
}

/// A Find Usages search: the symbol, where it was made, and its scope —
/// JetBrains "Recent Find Usages" lists these and runs one again.
#[derive(Clone)]
pub struct UsageSearch {
    pub symbol: String,
    pub origin: (PathBuf, usize, usize),
    pub scope: UsageScope,
}

/// How many searches "Recent Find Usages" keeps.
const RECENT_SEARCHES: usize = 20;

static RECENT: Mutex<Vec<UsageSearch>> = Mutex::new(Vec::new());

/// Record a search, newest first, dropping an earlier one for the same place.
pub fn remember_search(symbol: String, origin: (PathBuf, usize, usize), scope: UsageScope) {
    let mut recent = RECENT.lock().unwrap_or_else(|e| e.into_inner());
    recent.retain(|s| !(s.symbol == symbol && s.origin == origin && s.scope == scope));
    recent.insert(
        0,
        UsageSearch {
            symbol,
            origin,
            scope,
        },
    );
    recent.truncate(RECENT_SEARCHES);
}

pub fn recent_searches() -> Vec<UsageSearch> {
    RECENT.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// One usage.
pub struct UsageHit {
    pub path: PathBuf,
    /// 0-based line and char column.
    pub line: usize,
    pub col: usize,
    pub text: String,
    pub kind: UsageKind,
    pub test: bool,
    pub generated: bool,
    pub module: Option<PathBuf>,
    pub member: Option<String>,
}

enum Row {
    Header { depth: usize, label: String },
    Hit { depth: usize, hit: usize },
}

pub struct UsagesView {
    symbol: String,
    root: PathBuf,
    hits: Vec<UsageHit>,
    excluded: HashSet<usize>,
    removed: HashSet<usize>,
    rows: Vec<Row>,
    selected: usize,
    scroll: usize,
    viewport: usize,
    /// Where the search was made, for Rerun.
    origin: Option<(PathBuf, usize, usize)>,
}

/// A location the references request returned: file, 0-based line and char
/// column, and the length of the symbol there.
pub struct UsageLocation {
    pub path: PathBuf,
    pub line: usize,
    pub col: usize,
    pub len: usize,
}

impl UsagesView {
    /// Classify `locations` and build the view. Each file's text comes from
    /// its buffer when it is open, else from disk, read once.
    pub fn new(
        editor: &Editor,
        symbol: String,
        locations: Vec<UsageLocation>,
        origin: Option<(PathBuf, usize, usize)>,
    ) -> Self {
        let root = zmax_loader::find_workspace().0;
        let loader = editor.syn_loader.load_full();
        let mut by_file: Vec<(PathBuf, Vec<UsageLocation>)> = Vec::new();
        for location in locations {
            match by_file.iter_mut().find(|(p, _)| *p == location.path) {
                Some((_, list)) => list.push(location),
                None => by_file.push((location.path.clone(), vec![location])),
            }
        }
        by_file.sort_by(|a, b| a.0.cmp(&b.0));
        let mut hits = Vec::new();
        for (path, mut locations) in by_file {
            locations.sort_by_key(|l| (l.line, l.col));
            let text = editor
                .document_by_path(&path)
                .map(|doc| doc.text().clone())
                .or_else(|| {
                    std::fs::read_to_string(&path)
                        .ok()
                        .map(|s| Rope::from_str(&s))
                })
                .unwrap_or_default();
            let language = loader.language_for_filename(&path);
            let comments: Vec<String> = language
                .and_then(|lang| loader.language(lang).config().comment_tokens.clone())
                .unwrap_or_default();
            let comments: Vec<&str> = comments.iter().map(String::as_str).collect();
            let head: String = text.lines().take(5).map(|l| l.to_string()).collect();
            let generated =
                usage_kind::is_generated_path(&path) || usage_kind::has_generated_marker(&head);
            let test = usage_kind::is_test_path(&path);
            let module = usage_kind::module_root(&path, Path::is_file);
            let members = language
                .map(|lang| {
                    enclosing_members(&loader, lang, &text, locations.iter().map(|l| l.line))
                })
                .unwrap_or_else(|| vec![None; locations.len()]);
            for (location, member) in locations.into_iter().zip(members) {
                let line = text
                    .get_line(location.line)
                    .map(|l| l.to_string().trim_end_matches(['\n', '\r']).to_string())
                    .unwrap_or_default();
                hits.push(UsageHit {
                    kind: usage_kind::classify(&line, location.col, location.len, &comments),
                    path: location.path,
                    line: location.line,
                    col: location.col,
                    text: line,
                    test,
                    generated,
                    module: module.clone(),
                    member,
                });
            }
        }
        let mut view = UsagesView {
            symbol,
            root,
            hits,
            excluded: HashSet::new(),
            removed: HashSet::new(),
            rows: Vec::new(),
            selected: 0,
            scroll: 0,
            viewport: 1,
            origin,
        };
        view.build_rows();
        view
    }

    pub fn hit_count(&self) -> usize {
        self.hits.len() - self.removed.len()
    }

    /// The usages the filters keep.
    fn shown(&self, options: &UsageOptions, hit: &UsageHit) -> bool {
        (options.show_comments || hit.kind != UsageKind::Comment)
            && (options.show_imports || hit.kind != UsageKind::Import)
            && (options.show_generated || !hit.generated)
            && match options.access {
                Access::All => true,
                Access::Read => hit.kind == UsageKind::Read,
                Access::Write => hit.kind == UsageKind::Write,
            }
    }

    fn rel(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .display()
            .to_string()
    }

    /// The group labels a usage sits under, outermost first.
    fn groups(&self, options: &UsageOptions, hit: &UsageHit) -> Vec<String> {
        let mut groups = Vec::new();
        if options.by_type {
            groups.push(hit.kind.label().to_string());
        }
        if options.by_scope {
            groups.push(if hit.test { "Test" } else { "Production" }.to_string());
        }
        if options.by_module {
            let module = hit
                .module
                .as_deref()
                .map(|m| self.rel(m))
                .unwrap_or_default();
            let module = if module.is_empty() {
                "(root)".to_string()
            } else {
                module
            };
            if options.flatten_modules {
                groups.push(module);
            } else {
                // Nested modules: one level per path component of the module.
                groups.extend(module.split('/').map(str::to_owned));
            }
        }
        let rel = self.rel(&hit.path);
        let (dir, file) = match rel.rsplit_once('/') {
            Some((dir, file)) => (dir.to_string(), file.to_string()),
            None => (String::new(), rel.clone()),
        };
        if options.directory_tree {
            groups.extend(dir.split('/').filter(|c| !c.is_empty()).map(str::to_owned));
        } else if options.by_directory && !dir.is_empty() {
            groups.push(dir);
        }
        let file_label = if options.short_paths || options.by_directory || options.directory_tree {
            file
        } else {
            rel
        };
        groups.push(file_label);
        if options.by_member {
            if let Some(member) = &hit.member {
                groups.push(member.clone());
            }
        }
        groups
    }

    /// Lay the usages out under their group headers, keeping the selection on
    /// the same usage when it is still shown.
    fn build_rows(&mut self) {
        let options = options();
        let keep = self.current_hit_index();
        let mut order: Vec<(Vec<String>, usize)> = (0..self.hits.len())
            .filter(|i| !self.removed.contains(i) && self.shown(&options, &self.hits[*i]))
            .map(|i| (self.groups(&options, &self.hits[i]), i))
            .collect();
        // Stable: within a group, usages keep their file and line order.
        order.sort_by(|a, b| a.0.cmp(&b.0));
        let mut rows = Vec::new();
        let mut open: Vec<String> = Vec::new();
        for (groups, hit) in order {
            let common = open.iter().zip(&groups).take_while(|(a, b)| a == b).count();
            for (depth, label) in groups.iter().enumerate().skip(common) {
                rows.push(Row::Header {
                    depth,
                    label: label.clone(),
                });
            }
            rows.push(Row::Hit {
                depth: groups.len(),
                hit,
            });
            open = groups;
        }
        self.rows = rows;
        self.selected = keep
            .and_then(|k| {
                self.rows
                    .iter()
                    .position(|r| matches!(r, Row::Hit { hit, .. } if *hit == k))
            })
            .or_else(|| self.rows.iter().position(|r| matches!(r, Row::Hit { .. })))
            .unwrap_or(0);
    }

    fn current_hit_index(&self) -> Option<usize> {
        match self.rows.get(self.selected)? {
            Row::Hit { hit, .. } => Some(*hit),
            Row::Header { .. } => None,
        }
    }

    fn move_cursor(&mut self, delta: isize) {
        let mut i = self.selected as isize;
        loop {
            i += delta;
            if i < 0 || i >= self.rows.len() as isize {
                return;
            }
            if matches!(self.rows[i as usize], Row::Hit { .. }) {
                self.selected = i as usize;
                return;
            }
        }
    }

    /// The first usage of the next (`dir` 1) or previous (-1) innermost group.
    fn move_group(&mut self, dir: isize) {
        let mut i = self.selected as isize;
        loop {
            i += dir;
            if i < 0 || i >= self.rows.len() as isize {
                return;
            }
            if matches!(self.rows[i as usize], Row::Header { .. }) {
                if let Some(j) = (i as usize + 1..self.rows.len())
                    .find(|&j| matches!(self.rows[j], Row::Hit { .. }))
                {
                    self.selected = j;
                }
                return;
            }
        }
    }

    /// JetBrains "Exclude" / "Include": the usage stays in the list, greyed,
    /// and is left out of what is done with the results.
    pub fn toggle_exclude(&mut self) -> Option<bool> {
        let hit = self.current_hit_index()?;
        let excluded = !self.excluded.remove(&hit);
        if excluded {
            self.excluded.insert(hit);
        }
        Some(excluded)
    }

    /// JetBrains "Include": the usage takes part again.
    pub fn include(&mut self) -> bool {
        self.current_hit_index()
            .is_some_and(|hit| self.excluded.remove(&hit))
    }

    /// JetBrains "Remove": drop the usage from the list, the cursor moving to
    /// the next one.
    pub fn remove(&mut self) -> bool {
        let Some(hit) = self.current_hit_index() else {
            return false;
        };
        self.removed.insert(hit);
        let next = self.rows[self.selected + 1..].iter().find_map(|r| match r {
            Row::Hit { hit, .. } => Some(*hit),
            _ => None,
        });
        self.build_rows();
        if let Some(next) = next {
            if let Some(i) = self
                .rows
                .iter()
                .position(|r| matches!(r, Row::Hit { hit, .. } if *hit == next))
            {
                self.selected = i;
            }
        }
        true
    }

    /// Re-lay the rows after the options changed.
    pub fn refresh(&mut self) {
        self.build_rows();
    }

    /// Go to the usage under the cursor, closing the view.
    fn goto(&self) -> Option<Callback> {
        let hit = &self.hits[self.current_hit_index()?];
        let (path, line, col) = (hit.path.clone(), hit.line, hit.col);
        Some(Box::new(
            move |compositor: &mut Compositor, cx: &mut Context| {
                compositor.pop();
                crate::commands::open_at(
                    cx.editor,
                    &path,
                    line,
                    col,
                    zmax_view::editor::Action::Replace,
                    false,
                );
            },
        ))
    }

    /// JetBrains "Rerun": search again from where the search was made.
    pub fn rerun(&self) -> Option<Callback> {
        let (path, line, col) = self.origin.clone()?;
        Some(Box::new(
            move |compositor: &mut Compositor, cx: &mut Context| {
                compositor.pop();
                crate::commands::open_at(
                    cx.editor,
                    &path,
                    line,
                    col,
                    zmax_view::editor::Action::Replace,
                    false,
                );
                crate::commands::menu_run(compositor, cx, &crate::commands::find_usages);
            },
        ))
    }
}

/// For each of `lines`, the header of the innermost function or class around
/// it, by the language's text objects.
fn enclosing_members(
    loader: &zmax_core::syntax::Loader,
    language: zmax_core::Language,
    text: &Rope,
    lines: impl Iterator<Item = usize>,
) -> Vec<Option<String>> {
    let lines: Vec<usize> = lines.collect();
    let slice = text.slice(..);
    let Ok(syntax) = zmax_core::Syntax::new(slice, language, loader) else {
        return vec![None; lines.len()];
    };
    let Some(query) = loader.textobject_query(language) else {
        return vec![None; lines.len()];
    };
    let root = syntax.tree().root_node();
    let spans: Vec<(usize, usize)> = query
        .capture_nodes_any(&["function.around", "class.around"], &root, slice)
        .map(|nodes| {
            nodes
                .map(|n| {
                    let r = n.byte_range();
                    (
                        slice.byte_to_line(r.start),
                        slice.byte_to_line(r.end.min(slice.len_bytes())),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    lines
        .into_iter()
        .map(|line| {
            spans
                .iter()
                .filter(|(start, end)| *start <= line && line <= *end)
                .min_by_key(|(start, end)| end - start)
                .map(|(start, _)| {
                    slice
                        .line(*start)
                        .to_string()
                        .trim()
                        .trim_end_matches('{')
                        .trim_end()
                        .to_string()
                })
        })
        .collect()
}

/// Flip a usage option from inside the view.
fn toggle(f: impl FnOnce(&mut UsageOptions)) {
    set_options(f);
}

impl Component for UsagesView {
    fn handle_event(&mut self, event: &Event, _cx: &mut Context) -> EventResult {
        let Event::Key(key) = event else {
            return EventResult::Ignored(None);
        };
        let close: Callback = Box::new(|compositor: &mut Compositor, _| {
            compositor.pop();
        });
        let mut relayout = true;
        match *key {
            key!('q') | key!(Esc) | ctrl!('c') => return EventResult::Consumed(Some(close)),
            key!('n') | key!('j') | key!(Down) => {
                self.move_cursor(1);
                relayout = false;
            }
            key!('p') | key!('k') | key!(Up) => {
                self.move_cursor(-1);
                relayout = false;
            }
            key!('}') => {
                self.move_group(1);
                relayout = false;
            }
            key!('{') => {
                self.move_group(-1);
                relayout = false;
            }
            key!(Enter) | key!('o') => return EventResult::Consumed(self.goto()),
            key!('r') => return EventResult::Consumed(self.rerun()),
            key!('x') => {
                self.toggle_exclude();
                relayout = false;
            }
            key!('d') => {
                self.remove();
                relayout = false;
            }
            key!('t') => toggle(|o| o.by_type = !o.by_type),
            key!('s') => toggle(|o| o.by_scope = !o.by_scope),
            key!('m') => toggle(|o| o.by_module = !o.by_module),
            key!('M') => toggle(|o| o.flatten_modules = !o.flatten_modules),
            key!('D') => toggle(|o| o.by_directory = !o.by_directory),
            key!('T') => toggle(|o| o.directory_tree = !o.directory_tree),
            key!('f') => toggle(|o| o.by_member = !o.by_member),
            key!('P') => toggle(|o| o.short_paths = !o.short_paths),
            key!('c') => toggle(|o| o.show_comments = !o.show_comments),
            key!('i') => toggle(|o| o.show_imports = !o.show_imports),
            key!('G') => toggle(|o| o.show_generated = !o.show_generated),
            key!('R') => toggle(|o| {
                o.access = if o.access == Access::Read {
                    Access::All
                } else {
                    Access::Read
                }
            }),
            key!('W') => toggle(|o| {
                o.access = if o.access == Access::Write {
                    Access::All
                } else {
                    Access::Write
                }
            }),
            _ => relayout = false,
        }
        if relayout {
            self.build_rows();
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, cx: &mut Context) {
        let theme = &cx.editor.theme;
        let mut bg = theme.get("ui.background");
        if cx.editor.config().transparent_background {
            bg.bg = None;
        }
        surface.clear_with(area, bg);
        if area.width < 8 || area.height < 3 {
            return;
        }
        let header = theme.get("ui.text.focus");
        let group = theme.get("ui.text.directory");
        let text = theme.get("ui.text");
        let dim = theme.get("ui.linenr");
        let selected = theme.get("ui.selection");

        let shown = self
            .rows
            .iter()
            .filter(|r| matches!(r, Row::Hit { .. }))
            .count();
        let title = format!(
            " Usages of {}   ({shown} shown of {}, {} excluded)",
            self.symbol,
            self.hit_count(),
            self.excluded.len()
        );
        surface.set_stringn(area.x, area.y, &title, area.width as usize, header);

        let body_y = area.y + 1;
        let body_h = area.height.saturating_sub(2) as usize;
        self.viewport = body_h;
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if body_h > 0 && self.selected >= self.scroll + body_h {
            self.scroll = self.selected + 1 - body_h;
        }
        for (i, row) in self.rows.iter().enumerate().skip(self.scroll).take(body_h) {
            let y = body_y + (i - self.scroll) as u16;
            match row {
                Row::Header { depth, label } => {
                    let line = format!("{}▾ {label}", "  ".repeat(*depth));
                    surface.set_stringn(area.x, y, &line, area.width as usize, group);
                }
                Row::Hit { depth, hit } => {
                    let usage = &self.hits[*hit];
                    let line = format!(
                        "{}{:>5}  {}",
                        "  ".repeat(*depth),
                        usage.line + 1,
                        usage.text.trim()
                    );
                    let style = if i == self.selected {
                        selected
                    } else if self.excluded.contains(hit) {
                        dim
                    } else {
                        text
                    };
                    surface.set_stringn(area.x, y, &line, area.width as usize, style);
                }
            }
        }
        if self.rows.is_empty() {
            surface.set_stringn(
                area.x,
                body_y,
                "(no usages pass the filters)",
                area.width as usize,
                dim,
            );
        }
        let footer = "n/p move  }/{ group  ⏎ go  x exclude  d remove  r rerun  t s m M D T f P group  c i G R W filter  q quit";
        surface.set_stringn(
            area.x,
            area.y + area.height - 1,
            footer,
            area.width as usize,
            dim,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(path: &str, line: usize, kind: UsageKind) -> UsageHit {
        UsageHit {
            path: PathBuf::from(path),
            line,
            col: 0,
            text: String::new(),
            kind,
            test: path.contains("tests/"),
            generated: false,
            module: None,
            member: None,
        }
    }

    fn view(hits: Vec<UsageHit>) -> UsagesView {
        let mut view = UsagesView {
            symbol: "x".into(),
            root: PathBuf::from("/w"),
            hits,
            excluded: HashSet::new(),
            removed: HashSet::new(),
            rows: Vec::new(),
            selected: 0,
            scroll: 0,
            viewport: 1,
            origin: None,
        };
        view.build_rows();
        view
    }

    fn headers(view: &UsagesView) -> Vec<(usize, String)> {
        view.rows
            .iter()
            .filter_map(|r| match r {
                Row::Header { depth, label } => Some((*depth, label.clone())),
                _ => None,
            })
            .collect()
    }

    // One test: the options are process-global.
    #[test]
    fn grouping_filters_and_removal() {
        let defaults = options();
        set_options(|o| {
            *o = UsageOptions {
                by_type: true,
                by_directory: false,
                directory_tree: false,
                ..defaults
            };
        });
        let mut v = view(vec![
            hit("/w/src/a.rs", 3, UsageKind::Call),
            hit("/w/src/b.rs", 1, UsageKind::Write),
            hit("/w/tests/t.rs", 5, UsageKind::Call),
        ]);
        assert_eq!(
            vec![
                (0, "Method call".to_string()),
                (1, "src/a.rs".to_string()),
                (1, "tests/t.rs".to_string()),
                (0, "Write access".to_string()),
                (1, "src/b.rs".to_string()),
            ],
            headers(&v)
        );

        set_options(|o| {
            o.by_type = false;
            o.by_scope = true;
            o.directory_tree = true;
        });
        v.refresh();
        assert_eq!(
            vec![
                (0, "Production".to_string()),
                (1, "src".to_string()),
                (2, "a.rs".to_string()),
                (2, "b.rs".to_string()),
                (0, "Test".to_string()),
                (1, "tests".to_string()),
                (2, "t.rs".to_string()),
            ],
            headers(&v)
        );

        set_options(|o| o.access = Access::Write);
        v.refresh();
        assert_eq!(
            1,
            v.rows
                .iter()
                .filter(|r| matches!(r, Row::Hit { .. }))
                .count()
        );

        set_options(|o| o.access = Access::All);
        v.refresh();
        assert_eq!(Some(true), v.toggle_exclude());
        assert!(v.include());
        assert!(v.remove());
        assert_eq!(2, v.hit_count());

        set_options(|o| *o = defaults);
    }
}
