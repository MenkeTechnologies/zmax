//! Unified Preferences window — a JetBrains-style tabbed shell hosting every
//! configuration editor (Settings, Keymap, Color Scheme, Run Configs). A clickable
//! tab strip sits above the active tab's panel; each tab is a self-contained
//! `Component` the shell delegates render/events to.
//!
//! Switch tabs: click a tab · Ctrl-Tab / Ctrl-Shift-Tab · Esc closes the window.

use tui::buffer::Buffer as Surface;
use zemacs_view::{
    graphics::Rect,
    input::{KeyCode, KeyModifiers, MouseButton, MouseEventKind},
};

use crate::{
    compositor::{Component, Context, Event, EventResult},
    ui::{
        keymap_editor::KeymapEditor, run_config::RunConfigPanel, settings::SettingsPanel,
        theme_editor::ThemeEditor,
    },
};

const TABS: [&str; 4] = ["Settings", "Keymap", "Color Scheme", "Run Configs"];

pub struct PreferencesPanel {
    tab: usize,
    settings: SettingsPanel,
    keymap: KeymapEditor,
    theme: ThemeEditor,
    run: RunConfigPanel,
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
            tab_hits: Vec::new(),
        }
    }

    fn active(&mut self) -> &mut dyn Component {
        match self.tab {
            0 => &mut self.settings,
            1 => &mut self.keymap,
            2 => &mut self.theme,
            _ => &mut self.run,
        }
    }
}

impl Component for PreferencesPanel {
    fn handle_event(&mut self, event: &Event, cx: &mut Context) -> EventResult {
        if let Event::Key(k) = event {
            // Ctrl-Tab / Ctrl-Shift-Tab cycle tabs (clicks handled below).
            if k.code == KeyCode::Tab && k.modifiers.contains(KeyModifiers::CONTROL) {
                self.tab = if k.modifiers.contains(KeyModifiers::SHIFT) {
                    (self.tab + TABS.len() - 1) % TABS.len()
                } else {
                    (self.tab + 1) % TABS.len()
                };
                return EventResult::Consumed(None);
            }
        }
        if let Event::Mouse(ev) = event {
            if matches!(ev.kind, MouseEventKind::Down(MouseButton::Left)) {
                if let Some(&(_, _, _, t)) = self
                    .tab_hits
                    .iter()
                    .find(|&&(x0, x1, r, _)| ev.row == r && ev.column >= x0 && ev.column < x1)
                {
                    self.tab = t;
                    return EventResult::Consumed(None);
                }
            }
        }
        // Everything else → the active tab (it closes the window on Esc in list mode).
        self.active().handle_event(event, cx)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        use crate::ui::rat::{render, to_rat_style};
        use ratatui::style::Modifier as RMod;
        use ratatui::text::Span;
        use ratatui::widgets::Paragraph;

        self.tab_hits.clear();
        surface.clear_with(area, ctx.editor.theme.get("ui.background"));

        let theme = &ctx.editor.theme;
        let active_st = to_rat_style(theme.get("ui.text.focus")).add_modifier(RMod::BOLD | RMod::REVERSED);
        let idle_st = to_rat_style(theme.get("comment"));

        // tab strip on the top row
        let mut x = area.x + 1;
        for (i, name) in TABS.iter().enumerate() {
            let label = format!(" {name} ");
            let w = label.chars().count() as u16;
            let st = if i == self.tab { active_st } else { idle_st };
            render(Paragraph::new(Span::styled(label, st)), Rect::new(x, area.y, w, 1), surface);
            self.tab_hits.push((x, x + w, area.y, i));
            x += w + 1;
        }
        // hint, right-aligned
        let hint = "Ctrl-Tab ↹  ";
        let hw = hint.chars().count() as u16;
        if area.width > hw + 4 {
            render(
                Paragraph::new(Span::styled(hint, idle_st)),
                Rect::new(area.x + area.width - hw - 1, area.y, hw, 1),
                surface,
            );
        }

        // active tab fills the rest
        let body = Rect::new(area.x, area.y + 1, area.width, area.height.saturating_sub(1));
        self.active().render(body, surface, ctx);
    }
}
