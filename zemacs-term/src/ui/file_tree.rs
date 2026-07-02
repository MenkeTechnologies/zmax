//! TUI project file-tree sidebar (the first IDE panel of the terminal workbench).
//!
//! Rendered inside `EditorView` as a left strip (toggle with F2). Lazy directory
//! expansion, keyboard navigation, and opening files directly via the in-process
//! editor — no PTY round-trip, since this runs inside zemacs itself.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use tui::buffer::Buffer as Surface;
use zemacs_view::{
    graphics::Rect, input::KeyEvent, keyboard::KeyCode, keyboard::KeyModifiers, Theme,
};

/// Result of a key press handled by the tree.
pub enum TreeAction {
    None,
    Open(PathBuf),
    Close,
}

struct Row {
    path: PathBuf,
    name: String,
    depth: usize,
    is_dir: bool,
    expanded: bool,
}

pub struct FileTree {
    root: PathBuf,
    expanded: HashSet<PathBuf>,
    rows: Vec<Row>,
    selected: usize,
    scroll: usize,
    /// Speed-search query (JetBrains-style). Empty unless filtering.
    filter: String,
    /// True while the user is typing into the speed-search field.
    filtering: bool,
    /// Rows the filter box occupies at the top of the tree area (0 when hidden).
    /// Recorded each render so mouse hit-testing offsets the list correctly.
    list_offset: u16,
    /// Whether dotfiles are shown (from `editor.file-explorer.hidden`, inverted).
    /// Defaults to showing them; the owning view syncs this from config.
    show_hidden: bool,
    /// Cache of `read_dir` listings per directory, so rebuilding the visible tree
    /// (on every expand/collapse and every speed-search keystroke) doesn't re-hit
    /// the disk for directories already read. Cleared by `refresh` (the file
    /// watcher) and whenever `show_hidden` changes.
    dir_cache: HashMap<PathBuf, Vec<(PathBuf, String, bool)>>,
}

impl FileTree {
    pub fn new(root: PathBuf) -> Self {
        let mut tree = Self {
            root: root.clone(),
            expanded: HashSet::new(),
            rows: Vec::new(),
            selected: 0,
            scroll: 0,
            filter: String::new(),
            filtering: false,
            list_offset: 0,
            show_hidden: true,
            dir_cache: HashMap::new(),
        };
        tree.expanded.insert(root);
        tree.rebuild();
        tree
    }

    /// Set whether dotfiles are shown (from `editor.file-explorer.hidden`),
    /// rebuilding the tree if the value changed. Called by the owning view each
    /// render so config edits take effect live.
    pub fn set_show_hidden(&mut self, show: bool) {
        if self.show_hidden != show {
            self.show_hidden = show;
            self.dir_cache.clear();
            self.rebuild();
        }
    }

    /// Directory children, memoized. Reads and caches `dir` on a miss; on a hit
    /// returns the cached listing (cheap clone) so a rebuild walks RAM, not disk.
    fn cached_children(
        cache: &mut HashMap<PathBuf, Vec<(PathBuf, String, bool)>>,
        dir: &Path,
        show_hidden: bool,
    ) -> Vec<(PathBuf, String, bool)> {
        if let Some(v) = cache.get(dir) {
            return v.clone();
        }
        let v = Self::read_dir_sorted(dir, show_hidden);
        cache.insert(dir.to_path_buf(), v.clone());
        v
    }

    /// The workspace root the tree is rooted at (for root-level New actions).
    pub fn root_path(&self) -> PathBuf {
        self.root.clone()
    }

    /// The set of currently-expanded directory paths (for session persistence).
    pub fn expanded_paths(&self) -> Vec<PathBuf> {
        self.expanded.iter().cloned().collect()
    }

    /// Restore a persisted set of expanded directories, then rebuild the view.
    /// The project root stays expanded regardless. Paths that no longer exist are
    /// harmless (they just won't render).
    pub fn set_expanded_paths(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        self.expanded.extend(paths);
        self.expanded.insert(self.root.clone());
        self.rebuild();
    }

    /// Directory entries, dirs first, then case-insensitive by name. Dotfiles are
    /// included unless `show_hidden` is false (`editor.file-explorer.hidden`).
    fn read_dir_sorted(dir: &Path, show_hidden: bool) -> Vec<(PathBuf, String, bool)> {
        let mut entries: Vec<(PathBuf, String, bool)> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| {
                let path = e.path();
                let is_dir = path.is_dir();
                let name = e.file_name().to_string_lossy().into_owned();
                (path, name, is_dir)
            })
            .filter(|(_, name, _)| show_hidden || !name.starts_with('.'))
            .collect();
        entries.sort_by(|a, b| {
            b.2.cmp(&a.2)
                .then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase()))
        });
        entries
    }

    /// Re-read the directory tree from disk (preserving expand/selection state).
    /// Called by the filesystem watcher when files change on disk, so the cache
    /// is dropped to pick up the on-disk changes.
    pub fn refresh(&mut self) {
        self.dir_cache.clear();
        self.rebuild();
    }

    fn rebuild(&mut self) {
        let show_hidden = self.show_hidden;
        fn walk(
            cache: &mut HashMap<PathBuf, Vec<(PathBuf, String, bool)>>,
            dir: &Path,
            depth: usize,
            expanded: &HashSet<PathBuf>,
            out: &mut Vec<Row>,
            show_hidden: bool,
        ) {
            for (path, name, is_dir) in FileTree::cached_children(cache, dir, show_hidden) {
                let exp = is_dir && expanded.contains(&path);
                out.push(Row {
                    path: path.clone(),
                    name,
                    depth,
                    is_dir,
                    expanded: exp,
                });
                if exp {
                    walk(cache, &path, depth + 1, expanded, out, show_hidden);
                }
            }
        }
        // Speed-search: descend into every directory (ignoring expand state) and
        // keep only the paths leading to a name that matches the query. Directories
        // on a matching path render auto-expanded. Depth-capped to stay snappy.
        // fzf-style fuzzy match: every char of `q` (already lowercased) appears in
        // `name` in order (a subsequence), case-insensitively.
        fn fuzzy(name: &str, q: &str) -> bool {
            if q.is_empty() {
                return true;
            }
            let mut qc = q.chars().peekable();
            for nc in name.chars().flat_map(|c| c.to_lowercase()) {
                match qc.peek() {
                    Some(&want) if nc == want => {
                        qc.next();
                    }
                    Some(_) => {}
                    None => break,
                }
            }
            qc.peek().is_none()
        }
        fn walk_filtered(
            cache: &mut HashMap<PathBuf, Vec<(PathBuf, String, bool)>>,
            dir: &Path,
            depth: usize,
            q: &str,
            out: &mut Vec<Row>,
            show_hidden: bool,
        ) -> bool {
            if depth > 16 {
                return false;
            }
            let mut any = false;
            for (path, name, is_dir) in FileTree::cached_children(cache, dir, show_hidden) {
                if is_dir {
                    let name_match = fuzzy(&name, q);
                    let mut kids = Vec::new();
                    let child_match =
                        walk_filtered(cache, &path, depth + 1, q, &mut kids, show_hidden);
                    if name_match || child_match {
                        out.push(Row {
                            path,
                            name,
                            depth,
                            is_dir: true,
                            expanded: true,
                        });
                        out.append(&mut kids);
                        any = true;
                    }
                } else if fuzzy(&name, q) {
                    out.push(Row {
                        path,
                        name,
                        depth,
                        is_dir: false,
                        expanded: false,
                    });
                    any = true;
                }
            }
            any
        }

        let mut rows = Vec::new();
        let q = self.filter.to_lowercase();
        let root = self.root.clone();
        let expanded = std::mem::take(&mut self.expanded);
        if q.is_empty() {
            walk(
                &mut self.dir_cache,
                &root,
                0,
                &expanded,
                &mut rows,
                show_hidden,
            );
        } else {
            walk_filtered(&mut self.dir_cache, &root, 0, &q, &mut rows, show_hidden);
        }
        self.expanded = expanded;
        self.rows = rows;
        if self.selected >= self.rows.len() {
            self.selected = self.rows.len().saturating_sub(1);
        }
    }

    /// The (path, is_dir) at a visible list row, without changing selection or
    /// expand state. Used by the right-click context menu.
    pub fn path_at_row(&self, list_row: usize) -> Option<(std::path::PathBuf, bool)> {
        // Rows above the list (the filter box) aren't entries.
        let list_row = list_row.checked_sub(self.list_offset as usize)?;
        let idx = self.scroll + list_row;
        self.rows.get(idx).map(|r| (r.path.clone(), r.is_dir))
    }

    /// Mouse click on the visible list row `list_row` (0-based, below the header):
    /// select it, then toggle a directory or open a file.
    pub fn click_row(&mut self, list_row: usize) -> TreeAction {
        // A click on the filter box (the rows above the list) isn't a row hit.
        let Some(list_row) = list_row.checked_sub(self.list_offset as usize) else {
            return TreeAction::None;
        };
        let idx = self.scroll + list_row;
        if idx >= self.rows.len() {
            return TreeAction::None;
        }
        self.selected = idx;
        let row = &self.rows[idx];
        if row.is_dir {
            if self.expanded.contains(&row.path) {
                self.expanded.remove(&row.path);
            } else {
                self.expanded.insert(row.path.clone());
            }
            self.rebuild();
            TreeAction::None
        } else {
            TreeAction::Open(row.path.clone())
        }
    }

    /// Move the selection one row (mouse wheel).
    pub fn scroll_sel(&mut self, down: bool) {
        if down {
            if self.selected + 1 < self.rows.len() {
                self.selected += 1;
            }
        } else {
            self.selected = self.selected.saturating_sub(1);
        }
    }

    /// True while the speed-search field is capturing keystrokes — the IDE routes
    /// every key straight here so chrome shortcuts (z/</>/Esc/Tab) become text.
    pub fn is_filtering(&self) -> bool {
        self.filtering
    }

    /// Collapse every directory back to the root (IDE "Collapse All").
    pub fn collapse_all(&mut self) {
        self.expanded.clear();
        self.expanded.insert(self.root.clone());
        self.selected = 0;
        self.scroll = 0;
        self.rebuild();
    }

    /// Expand every code directory under the root — the JetBrains "Expand All"
    /// toolbar button. Bounded so a huge tree can't stall. Dotfile directories
    /// (`.git`, `.github`, …) and heavy build/dependency dirs are NEVER descended
    /// into here — recursively walking `.git/objects` would freeze the UI — even
    /// though dotfiles are still listed in the tree and can be expanded by hand.
    pub fn expand_all(&mut self) {
        fn collect(dir: &Path, out: &mut HashSet<PathBuf>, budget: &mut usize) {
            if *budget == 0 {
                return;
            }
            let Ok(rd) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in rd.flatten() {
                let p = entry.path();
                if !p.is_dir() {
                    continue;
                }
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with('.')
                    || matches!(name.as_ref(), "target" | "node_modules" | "dist" | "build")
                {
                    continue;
                }
                out.insert(p.clone());
                *budget -= 1;
                collect(&p, out, budget);
            }
        }
        let mut budget = 5000usize;
        let root = self.root.clone();
        collect(&root, &mut self.expanded, &mut budget);
        self.expanded.insert(root);
        self.rebuild();
    }

    /// Reveal `path` in the tree (JetBrains "Select Opened File"): expand every
    /// ancestor directory down to the file and move the selection onto it. Any
    /// active speed-search filter is cleared first. No-op if it's outside root.
    pub fn reveal(&mut self, path: &Path) {
        self.filtering = false;
        self.filter.clear();
        // Expand each ancestor from the file's parent up to (and including) root.
        let mut cur = path.parent();
        while let Some(dir) = cur {
            if dir != self.root && !dir.starts_with(&self.root) {
                break;
            }
            self.expanded.insert(dir.to_path_buf());
            if dir == self.root {
                break;
            }
            cur = dir.parent();
        }
        self.rebuild();
        if let Some(idx) = self.rows.iter().position(|r| r.path == path) {
            self.selected = idx;
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> TreeAction {
        // Speed-search input mode: keystrokes build the query until Esc/Enter.
        if self.filtering {
            match key.code {
                KeyCode::Esc => {
                    self.filtering = false;
                    self.filter.clear();
                    self.selected = 0;
                    self.scroll = 0;
                    self.rebuild();
                    return TreeAction::None;
                }
                KeyCode::Enter => {
                    // Lock in the filter, leave the results; open if a file is selected.
                    self.filtering = false;
                    if let Some(row) = self.rows.get(self.selected) {
                        if !row.is_dir {
                            return TreeAction::Open(row.path.clone());
                        }
                    }
                    return TreeAction::None;
                }
                KeyCode::Backspace => {
                    self.filter.pop();
                    self.selected = 0;
                    self.scroll = 0;
                    self.rebuild();
                    return TreeAction::None;
                }
                KeyCode::Down => {
                    if self.selected + 1 < self.rows.len() {
                        self.selected += 1;
                    }
                    return TreeAction::None;
                }
                KeyCode::Up => {
                    self.selected = self.selected.saturating_sub(1);
                    return TreeAction::None;
                }
                // Emacs-style navigation while typing: C-n/C-j down, C-p/C-k up.
                KeyCode::Char('n') | KeyCode::Char('j')
                    if key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    if self.selected + 1 < self.rows.len() {
                        self.selected += 1;
                    }
                    return TreeAction::None;
                }
                KeyCode::Char('p') | KeyCode::Char('k')
                    if key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    self.selected = self.selected.saturating_sub(1);
                    return TreeAction::None;
                }
                KeyCode::Char(c) => {
                    self.filter.push(c);
                    self.selected = 0;
                    self.scroll = 0;
                    self.rebuild();
                    return TreeAction::None;
                }
                _ => return TreeAction::None,
            }
        }
        match key.code {
            KeyCode::Char('/') => {
                self.filtering = true;
                self.filter.clear();
                self.selected = 0;
                self.scroll = 0;
                self.rebuild();
                TreeAction::None
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if self.selected + 1 < self.rows.len() {
                    self.selected += 1;
                }
                TreeAction::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                TreeAction::None
            }
            // vim-style jumps to the first / last visible row.
            KeyCode::Char('g') | KeyCode::Home => {
                self.selected = 0;
                TreeAction::None
            }
            KeyCode::Char('G') | KeyCode::End => {
                self.selected = self.rows.len().saturating_sub(1);
                TreeAction::None
            }
            // Collapse the whole tree back to the project root.
            KeyCode::Char('c') => {
                self.collapse_all();
                TreeAction::None
            }
            KeyCode::Esc => TreeAction::Close,
            // Left: collapse an expanded dir, else jump to the parent directory.
            KeyCode::Char('h') | KeyCode::Left => {
                if let Some(row) = self.rows.get(self.selected) {
                    if row.is_dir && self.expanded.contains(&row.path) {
                        self.expanded.remove(&row.path);
                        self.rebuild();
                    } else if let Some(parent) = row.path.parent().map(|p| p.to_path_buf()) {
                        if let Some(idx) = self.rows.iter().position(|r| r.path == parent) {
                            self.selected = idx;
                        }
                    }
                }
                TreeAction::None
            }
            // Right: expand a collapsed dir; on an already-expanded dir descend to
            // the first child; on a file, open it.
            KeyCode::Char('l') | KeyCode::Right => {
                if let Some(row) = self.rows.get(self.selected) {
                    if row.is_dir {
                        if self.expanded.contains(&row.path) {
                            if self.selected + 1 < self.rows.len() {
                                self.selected += 1;
                            }
                        } else {
                            self.expanded.insert(row.path.clone());
                            self.rebuild();
                        }
                        TreeAction::None
                    } else {
                        TreeAction::Open(row.path.clone())
                    }
                } else {
                    TreeAction::None
                }
            }
            // Enter: toggle a dir, open a file.
            KeyCode::Enter => {
                if let Some(row) = self.rows.get(self.selected) {
                    if row.is_dir {
                        if self.expanded.contains(&row.path) {
                            self.expanded.remove(&row.path);
                        } else {
                            self.expanded.insert(row.path.clone());
                        }
                        self.rebuild();
                        TreeAction::None
                    } else {
                        TreeAction::Open(row.path.clone())
                    }
                } else {
                    TreeAction::None
                }
            }
            _ => TreeAction::None,
        }
    }

    /// Render just the tree rows into `area` (the Ide draws the drawer header above this).
    pub fn render(&mut self, area: Rect, surface: &mut Surface, theme: &Theme) {
        surface.clear_with(area, theme.get("ui.background"));
        if area.width == 0 || area.height == 0 {
            return;
        }

        // fzf-style speed-search: a bordered ratatui filter box at the top of the
        // tree while active (`/` opens it; keystrokes fuzzy-filter the rows live).
        let mut area = area;
        self.list_offset = 0;
        if self.filtering || !self.filter.is_empty() {
            use crate::ui::rat::to_rat_style;
            use ratatui::text::{Line, Span};
            use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

            let box_h = 3u16.min(area.height);
            let box_area = Rect::new(area.x, area.y, area.width, box_h);
            let focused = self.filtering;
            let border_style = to_rat_style(theme.get(if focused {
                "ui.text.focus"
            } else {
                "ui.window"
            }));
            let cursor = if focused { "▏" } else { "" };
            let hint = if self.rows.is_empty() && !self.filter.is_empty() {
                " · no matches"
            } else {
                ""
            };
            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(border_style)
                .title(Span::styled(
                    format!(" 🔍 filter{hint} "),
                    to_rat_style(theme.get("keyword")),
                ))
                .style(to_rat_style(theme.get("ui.background")));
            let content = Line::from(vec![Span::styled(
                format!(" {}{cursor}", self.filter),
                to_rat_style(theme.get("ui.text.focus")),
            )]);
            let para = Paragraph::new(content).block(block);
            crate::ui::rat::render(para, box_area, surface);
            self.list_offset = box_h;

            area = Rect::new(
                area.x,
                area.y + box_h,
                area.width,
                area.height.saturating_sub(box_h),
            );
            if area.height == 0 {
                return;
            }
        }

        let height = area.height as usize;

        // keep selection in view
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + height {
            self.scroll = self.selected + 1 - height;
        }

        let base = theme.get("ui.text");
        let dir_style = theme.get("function");
        let sel = theme.get("ui.selection");

        for i in 0..height {
            let idx = self.scroll + i;
            if idx >= self.rows.len() {
                break;
            }
            let row = &self.rows[idx];
            let y = area.y + i as u16;
            if idx == self.selected {
                surface.set_style(Rect::new(area.x, y, area.width, 1), sel);
            }
            let indent = "  ".repeat(row.depth);
            // Disclosure triangle + nerd-font filetype/folder glyph.
            let text = if row.is_dir {
                let arrow = if row.expanded { '▾' } else { '▸' };
                let folder = crate::ui::icons::folder_icon(row.expanded);
                format!("{indent}{arrow} {folder} {}", row.name)
            } else {
                let glyph = crate::ui::icons::file_icon(&row.name);
                format!("{indent}  {glyph} {}", row.name)
            };
            let style = if row.is_dir { dir_style } else { base };
            surface.set_stringn(area.x, y, &text, area.width as usize, style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zemacs_view::input::KeyModifiers;

    fn key(c: char) -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char(c),
            modifiers: KeyModifiers::empty(),
        }
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char(c),
            modifiers: KeyModifiers::CONTROL,
        }
    }

    #[test]
    fn speed_search_filters_to_matching_paths() {
        // Build a throwaway project tree on disk.
        let root = std::env::temp_dir().join(format!("zemacs_ft_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("alpha.rs"), "").unwrap();
        std::fs::write(root.join("beta.txt"), "").unwrap();
        std::fs::write(root.join("sub").join("gamma_alpha.rs"), "").unwrap();
        std::fs::write(root.join("sub").join("delta.txt"), "").unwrap();

        let mut tree = FileTree::new(root.clone());

        // Type "/alpha": only the two *alpha* files and their ancestor dir survive.
        tree.handle_key(key('/'));
        assert!(tree.is_filtering());
        for c in "alpha".chars() {
            tree.handle_key(key(c));
        }
        let names: Vec<&str> = tree.rows.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"alpha.rs"), "rows: {names:?}");
        assert!(names.contains(&"gamma_alpha.rs"), "rows: {names:?}");
        assert!(names.contains(&"sub"), "ancestor dir kept: {names:?}");
        assert!(!names.contains(&"beta.txt"), "non-match dropped: {names:?}");
        assert!(
            !names.contains(&"delta.txt"),
            "non-match dropped: {names:?}"
        );

        // Esc clears the filter and restores the full (collapsed) tree.
        tree.handle_key(KeyEvent {
            code: KeyCode::Esc,
            modifiers: KeyModifiers::empty(),
        });
        assert!(!tree.is_filtering());
        let names: Vec<&str> = tree.rows.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"beta.txt"), "filter cleared: {names:?}");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dotfiles_shown_by_default_and_toggle_via_config() {
        let root = std::env::temp_dir().join(format!("zemacs_dot_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(".env"), "").unwrap();
        std::fs::write(root.join("visible.rs"), "").unwrap();

        // Default: dotfiles are shown.
        let mut tree = FileTree::new(root.clone());
        let names: Vec<&str> = tree.rows.iter().map(|r| r.name.as_str()).collect();
        assert!(
            names.contains(&".env"),
            "dotfile shown by default: {names:?}"
        );
        assert!(names.contains(&"visible.rs"), "rows: {names:?}");

        // Turning on `file-explorer.hidden` (show_hidden = false) hides them.
        tree.set_show_hidden(false);
        let names: Vec<&str> = tree.rows.iter().map(|r| r.name.as_str()).collect();
        assert!(
            !names.contains(&".env"),
            "dotfile hidden when configured: {names:?}"
        );
        assert!(
            names.contains(&"visible.rs"),
            "non-dotfile still shown: {names:?}"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn reveal_expands_ancestors_and_selects_file() {
        let root = std::env::temp_dir().join(format!("zemacs_rv_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("sub").join("deep")).unwrap();
        let target = root.join("sub").join("deep").join("buried.rs");
        std::fs::write(&target, "").unwrap();
        std::fs::write(root.join("top.rs"), "").unwrap();

        let mut tree = FileTree::new(root.clone());
        // Initially the nested dirs are collapsed, so the file isn't a row.
        assert!(!tree.rows.iter().any(|r| r.path == target));

        tree.reveal(&target);
        // Ancestors expanded → the buried file is now a visible, selected row.
        let sel = &tree.rows[tree.selected];
        assert_eq!(sel.path, target, "selection landed on revealed file");
        assert!(!sel.is_dir);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn speed_search_ctrl_nav_moves_selection() {
        let root = std::env::temp_dir().join(format!("zemacs_cn_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("alpha.rs"), "").unwrap();
        std::fs::write(root.join("alpha2.rs"), "").unwrap();
        std::fs::write(root.join("zeta.txt"), "").unwrap();

        let mut tree = FileTree::new(root.clone());
        tree.handle_key(key('/'));
        for c in "alpha".chars() {
            tree.handle_key(key(c));
        }
        assert!(
            tree.rows.len() >= 2,
            "two matches expected: {}",
            tree.rows.len()
        );
        assert_eq!(tree.selected, 0);
        // C-n / C-j move down, C-p / C-k move up — same as the Help filter.
        tree.handle_key(ctrl('n'));
        assert_eq!(tree.selected, 1);
        tree.handle_key(ctrl('k'));
        assert_eq!(tree.selected, 0);
        tree.handle_key(ctrl('j'));
        assert_eq!(tree.selected, 1);
        tree.handle_key(ctrl('p'));
        assert_eq!(tree.selected, 0);
        // A plain letter still types into the filter (not a navigation key).
        let before = tree.rows.len();
        tree.handle_key(key('x'));
        assert!(tree.rows.len() <= before, "plain key narrowed the filter");
        assert_eq!(tree.filter, "alphax");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn nav_jumps_and_collapse_all() {
        let root = std::env::temp_dir().join(format!("zemacs_nav_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("a.rs"), "").unwrap();
        std::fs::write(root.join("sub").join("b.rs"), "").unwrap();

        let mut tree = FileTree::new(root.clone());
        // Reveal expands "sub", so there are >2 rows.
        tree.reveal(&root.join("sub").join("b.rs"));
        let expanded_rows = tree.rows.len();
        assert!(expanded_rows >= 3, "rows: {expanded_rows}");

        // G → last row, g → first row.
        tree.handle_key(key('G'));
        assert_eq!(tree.selected, tree.rows.len() - 1);
        tree.handle_key(key('g'));
        assert_eq!(tree.selected, 0);

        // c → collapse all: only the root's direct children remain.
        tree.handle_key(key('c'));
        assert!(
            tree.rows.len() < expanded_rows,
            "collapsed: {}",
            tree.rows.len()
        );
        assert!(
            !tree.rows.iter().any(|r| r.name == "b.rs"),
            "nested file hidden"
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}
