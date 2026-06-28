//! TUI project file-tree sidebar (the first IDE panel of the terminal workbench).
//!
//! Rendered inside `EditorView` as a left strip (toggle with F2). Lazy directory
//! expansion, keyboard navigation, and opening files directly via the in-process
//! editor — no PTY round-trip, since this runs inside zemacs itself.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use tui::buffer::Buffer as Surface;
use zemacs_view::{graphics::Rect, input::KeyEvent, keyboard::KeyCode, Theme};

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
}

impl FileTree {
    pub fn new(root: PathBuf) -> Self {
        let mut tree = Self {
            root: root.clone(),
            expanded: HashSet::new(),
            rows: Vec::new(),
            selected: 0,
            scroll: 0,
        };
        tree.expanded.insert(root);
        tree.rebuild();
        tree
    }

    /// Directory entries, dirs first, then case-insensitive by name; dotfiles skipped.
    fn read_dir_sorted(dir: &Path) -> Vec<(PathBuf, String, bool)> {
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
            .filter(|(_, name, _)| !name.starts_with('.'))
            .collect();
        entries.sort_by(|a, b| {
            b.2.cmp(&a.2)
                .then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase()))
        });
        entries
    }

    fn rebuild(&mut self) {
        fn walk(dir: &Path, depth: usize, expanded: &HashSet<PathBuf>, out: &mut Vec<Row>) {
            for (path, name, is_dir) in FileTree::read_dir_sorted(dir) {
                let exp = is_dir && expanded.contains(&path);
                out.push(Row {
                    path: path.clone(),
                    name,
                    depth,
                    is_dir,
                    expanded: exp,
                });
                if exp {
                    walk(&path, depth + 1, expanded, out);
                }
            }
        }
        let mut rows = Vec::new();
        walk(&self.root, 0, &self.expanded, &mut rows);
        self.rows = rows;
        if self.selected >= self.rows.len() {
            self.selected = self.rows.len().saturating_sub(1);
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> TreeAction {
        match key.code {
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
            KeyCode::Esc => TreeAction::Close,
            KeyCode::Char('h') | KeyCode::Left => {
                if let Some(row) = self.rows.get(self.selected) {
                    if row.is_dir && self.expanded.contains(&row.path) {
                        self.expanded.remove(&row.path);
                        self.rebuild();
                    }
                }
                TreeAction::None
            }
            KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => {
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

    pub fn render(&mut self, area: Rect, surface: &mut Surface, theme: &Theme, focused: bool) {
        surface.clear_with(area, theme.get("ui.background"));
        if area.width == 0 || area.height == 0 {
            return;
        }

        // header
        let header_style = if focused {
            theme.get("ui.text.focus")
        } else {
            theme.get("comment")
        };
        surface.set_stringn(area.x, area.y, " PROJECT", area.width as usize, header_style);

        let list_y = area.y + 1;
        let height = area.height.saturating_sub(1) as usize;
        if height == 0 {
            return;
        }

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
            let y = list_y + i as u16;
            if idx == self.selected {
                surface.set_style(Rect::new(area.x, y, area.width, 1), sel);
            }
            let indent = "  ".repeat(row.depth);
            let icon = if row.is_dir {
                if row.expanded {
                    "▾ "
                } else {
                    "▸ "
                }
            } else {
                "  "
            };
            let text = format!("{indent}{icon}{}", row.name);
            let style = if row.is_dir { dir_style } else { base };
            surface.set_stringn(area.x, y, &text, area.width as usize, style);
        }
    }
}
