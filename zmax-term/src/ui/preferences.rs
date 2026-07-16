//! The IDE configuration **page** — a full-screen, tabbed workspace (NOT a modal):
//! a top tab strip (Settings · Keymap · Color Scheme · Run Configs · Help) with the
//! active page filling the rest of the screen. Each tab is a self-contained
//! `Component` the page delegates render/events to.
//!
//! Switch tabs: click a tab · Ctrl-Tab / Ctrl-Shift-Tab · `[` / `]` · Esc closes.

use tui::buffer::Buffer as Surface;
use zmax_view::{
    graphics::Rect,
    input::{KeyCode, KeyModifiers, MouseButton, MouseEventKind},
};

use crate::{
    compositor::{Component, Context, Event, EventResult},
    ui::{
        dashboard::DashboardPanel, help::HelpPanel, keymap_editor::KeymapEditor,
        run_config::RunConfigPanel, settings::SettingsPanel, theme_editor::ThemeEditor,
    },
};

const TABS: [&str; 6] = [
    "Settings",
    "Keymap",
    "Color Scheme",
    "Run Configs",
    "Help",
    "Dashboard",
];

pub struct PreferencesPanel {
    tab: usize,
    settings: SettingsPanel,
    keymap: KeymapEditor,
    theme: ThemeEditor,
    run: RunConfigPanel,
    help: HelpPanel,
    dashboard: DashboardPanel,
    tab_hits: Vec<(u16, u16, u16, usize)>,
}

impl PreferencesPanel {
    pub fn new(tab: usize) -> Self {
        Self {
            tab: tab.min(TABS.len() - 1),
            settings: SettingsPanel::new(),
            keymap: KeymapEditor::new(),
            theme: ThemeEditor::new(),
            run: RunConfigPanel::new(),
            help: HelpPanel::new(),
            dashboard: DashboardPanel::new(),
            tab_hits: Vec::new(),
        }
    }

    /// Open directly on the Help tab, pre-filtered to `filter` (fzf.vim `:Helptags`).
    pub fn new_help(filter: String) -> Self {
        let mut p = Self::new(4);
        p.help = HelpPanel::with_filter(filter);
        p
    }

    /// Open directly on the Settings tab, pre-filtered to `filter` (Emacs
    /// `customize-variable` / `customize-option` / `customize-apropos`).
    pub fn new_settings_filtered(filter: String) -> Self {
        let mut p = Self::new(0);
        p.settings = SettingsPanel::with_filter(filter);
        p
    }

    /// Open directly on the Settings tab, showing only settings changed from their
    /// default (Emacs `customize-unsaved` / `customize-changed`).
    pub fn new_settings_modified() -> Self {
        let mut p = Self::new(0);
        p.settings = SettingsPanel::with_modified_only();
        p
    }

    fn active(&mut self) -> &mut dyn Component {
        match self.tab {
            0 => &mut self.settings,
            1 => &mut self.keymap,
            2 => &mut self.theme,
            3 => &mut self.run,
            4 => &mut self.help,
            _ => &mut self.dashboard,
        }
    }

    fn cycle(&mut self, forward: bool) {
        self.tab = if forward {
            (self.tab + 1) % TABS.len()
        } else {
            (self.tab + TABS.len() - 1) % TABS.len()
        };
    }
}

impl Component for PreferencesPanel {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        if let Event::Key(k) = event {
            // Ctrl-Tab / Ctrl-Shift-Tab cycle tabs (works from any page).
            if k.code == KeyCode::Tab && k.modifiers.contains(KeyModifiers::CONTROL) {
                self.cycle(!k.modifiers.contains(KeyModifiers::SHIFT));
                return EventResult::Consumed(None);
            }
        }
        if let Event::Mouse(ev) = event {
            if matches!(ev.kind, MouseEventKind::Down(MouseButton::Left)) {
                if let Some(&(_, _, _, t)) = self
                    .tab_hits
                    .iter()
                    .find(|&&(r, x0, x1, _)| ev.row == r && ev.column >= x0 && ev.column < x1)
                {
                    self.tab = t;
                    return EventResult::Consumed(None);
                }
            }
        }
        // Everything else → the active page (it pops the layer on Esc in its list mode).
        self.active().handle_event(event, cx)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        use crate::ui::rat::{render, to_rat_style};
        use ratatui::style::Modifier as RMod;
        use ratatui::text::Span;
        use ratatui::widgets::Paragraph;

        self.tab_hits.clear();
        // vim `helpheight`: `:help` opens a window of this height. zmax's help
        // browser is a full-screen panel, so the option pins it to that many rows
        // at the bottom of the screen — vim's help split. Unset: full screen.
        let area = match crate::commands::typed::vim_opt_num("helpheight") {
            Some(rows) if rows > 0 && (rows as u16) < area.height => {
                area.clip_top(area.height - rows as u16)
            }
            _ => area,
        };
        let theme = &ctx.editor.theme;
        // `transparent-background`: drop the page fill so the terminal shows through.
        let mut page_bg = theme.get("ui.background");
        if ctx.editor.config().transparent_background {
            page_bg.bg = None;
        }
        surface.clear_with(area, page_bg);

        let bar_bg = theme.get("ui.statusline");
        let active_st = to_rat_style(theme.get("ui.text.focus")).add_modifier(RMod::BOLD);
        let idle_st = to_rat_style(theme.get("comment"));

        // Top tab strip (full-width bar). Active tab is highlighted; the rest dim.
        surface.clear_with(Rect::new(area.x, area.y, area.width, 1), bar_bg);
        let mut x = area.x + 1;
        for (i, name) in TABS.iter().enumerate() {
            let label = format!(" {name} ");
            let w = label.chars().count() as u16;
            if x + w >= area.x + area.width {
                break;
            }
            let st = if i == self.tab {
                to_rat_style(theme.get("ui.selection")).add_modifier(RMod::BOLD)
            } else {
                idle_st
            };
            render(
                Paragraph::new(Span::styled(label, st)),
                Rect::new(x, area.y, w, 1),
                surface,
            );
            self.tab_hits.push((area.y, x, x + w, i));
            x += w;
            // separator between tabs
            render(
                Paragraph::new(Span::styled("│", idle_st)),
                Rect::new(x, area.y, 1, 1),
                surface,
            );
            x += 1;
        }
        // right-aligned hint
        let hint = " Ctrl-Tab ↹  Esc close ";
        let hw = hint.chars().count() as u16;
        if area.width > hw + x.saturating_sub(area.x) {
            render(
                Paragraph::new(Span::styled(hint, idle_st)),
                Rect::new(area.x + area.width - hw, area.y, hw, 1),
                surface,
            );
        }
        let _ = active_st;

        // Active page fills the rest of the screen (it draws its own content frame).
        let content = Rect::new(
            area.x,
            area.y + 1,
            area.width,
            area.height.saturating_sub(1),
        );
        self.active().render(content, surface, ctx);
    }
}
