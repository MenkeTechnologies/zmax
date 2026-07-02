//! Preferences → **Dashboard**: a live system/process stats screen that doubles
//! as a tour of (nearly) every ratatui widget, all blitted onto the zemacs
//! `Surface` through [`crate::ui::rat`].
//!
//! Stats come from `sysinfo` (CPU, per-core load, memory/swap, processes,
//! disks, networks) and the editor's own process. The screen refreshes on a
//! ~1s cadence; while it's open it schedules its own redraws via
//! `zemacs_event::request_redraw` so the gauges/charts animate without input.
//!
//! Widgets on show: `Block`, `Paragraph`, `Tabs`, `Gauge`, `LineGauge`,
//! `Sparkline`, `BarChart`, `Chart`, `Canvas`, `Table`, `List`, `Scrollbar`,
//! and `Calendar` (Monthly).
//!
//! Keys: `Tab` / `[` / `]` switch Overview ⇄ Processes · `j`/`k`/PgUp/PgDn or the
//! wheel scroll the process table · `Esc` closes the Preferences page.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use sysinfo::{Disks, Networks, Pid, ProcessesToUpdate, System};
use tui::buffer::Buffer as Surface;
use zemacs_view::{
    graphics::Rect,
    input::{KeyCode, MouseEventKind},
};

use crate::compositor::{Component, Compositor, Context, Event, EventResult};
use crate::ui::rat::{render, render_stateful, to_rat_style};

/// How often the stats are re-sampled (also the animation frame interval).
const REFRESH: Duration = Duration::from_millis(1000);
/// History depth for the time-series widgets.
const HIST: usize = 120;

#[derive(Clone, Copy, PartialEq)]
enum View {
    Overview,
    Processes,
}

/// One row of the top-processes table.
struct ProcRow {
    pid: u32,
    name: String,
    cpu: f32,
    mem: u64,
}

pub struct DashboardPanel {
    sys: System,
    disks: Disks,
    nets: Networks,
    last_tick: Option<Instant>,
    cpu_hist: VecDeque<f64>,
    mem_hist: VecDeque<f64>,
    net_rx_hist: VecDeque<u64>,
    net_tx_hist: VecDeque<u64>,
    cores: Vec<f64>,
    procs: Vec<ProcRow>,
    view: View,
    proc_scroll: usize,
    pid: u32,
    /// Screen row of the tab strip + per-tab (x0, x1, view) hit ranges, for clicks.
    tab_row: u16,
    tab_hits: Vec<(u16, u16, View)>,
}

impl Default for DashboardPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl DashboardPanel {
    pub fn new() -> Self {
        Self {
            sys: System::new(),
            disks: Disks::new_with_refreshed_list(),
            nets: Networks::new_with_refreshed_list(),
            last_tick: None,
            cpu_hist: VecDeque::new(),
            mem_hist: VecDeque::new(),
            net_rx_hist: VecDeque::new(),
            net_tx_hist: VecDeque::new(),
            cores: Vec::new(),
            procs: Vec::new(),
            view: View::Overview,
            proc_scroll: 0,
            pid: std::process::id(),
            tab_row: 0,
            tab_hits: Vec::new(),
        }
    }

    /// Re-sample stats if the refresh interval has elapsed. Returns whether a
    /// sample was taken (used to schedule the next animation frame).
    fn tick(&mut self) -> bool {
        if self.last_tick.is_some_and(|t| t.elapsed() < REFRESH) {
            return false;
        }
        self.last_tick = Some(Instant::now());

        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        self.sys.refresh_processes(ProcessesToUpdate::All, true);
        self.nets.refresh(true);
        self.disks.refresh(true);

        push_f(&mut self.cpu_hist, self.sys.global_cpu_usage() as f64);
        let total = self.sys.total_memory().max(1);
        push_f(
            &mut self.mem_hist,
            self.sys.used_memory() as f64 / total as f64 * 100.0,
        );
        self.cores = self
            .sys
            .cpus()
            .iter()
            .map(|c| c.cpu_usage() as f64)
            .collect();

        let (mut rx, mut tx) = (0u64, 0u64);
        for (_n, d) in self.nets.iter() {
            rx += d.received();
            tx += d.transmitted();
        }
        push_u(&mut self.net_rx_hist, rx);
        push_u(&mut self.net_tx_hist, tx);

        let mut rows: Vec<ProcRow> = self
            .sys
            .processes()
            .iter()
            .map(|(pid, p)| ProcRow {
                pid: pid.as_u32(),
                name: p.name().to_string_lossy().into_owned(),
                cpu: p.cpu_usage(),
                mem: p.memory(),
            })
            .collect();
        rows.sort_by(|a, b| {
            b.cpu
                .partial_cmp(&a.cpu)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        rows.truncate(256);
        self.procs = rows;
        true
    }
}

impl Component for DashboardPanel {
    fn handle_event(&mut self, event: &Event, _cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(k) => *k,
            Event::Mouse(ev) => {
                use zemacs_view::input::MouseButton;
                match ev.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        // Click a tab in the strip to switch views.
                        if ev.row == self.tab_row {
                            if let Some(&(_, _, view)) = self
                                .tab_hits
                                .iter()
                                .find(|&&(x0, x1, _)| ev.column >= x0 && ev.column < x1)
                            {
                                if self.view != view {
                                    self.view = view;
                                    self.proc_scroll = 0;
                                }
                            }
                        }
                    }
                    MouseEventKind::ScrollDown => self.proc_scroll += 3,
                    MouseEventKind::ScrollUp => {
                        self.proc_scroll = self.proc_scroll.saturating_sub(3)
                    }
                    _ => {}
                }
                return EventResult::Consumed(None);
            }
            _ => return EventResult::Ignored(None),
        };

        let toggle = |v: View| match v {
            View::Overview => View::Processes,
            View::Processes => View::Overview,
        };
        match key.code {
            KeyCode::Esc => EventResult::Consumed(Some(Box::new(|c: &mut Compositor, _| {
                c.pop();
            }))),
            KeyCode::Tab | KeyCode::Char('[') | KeyCode::Char(']') => {
                self.view = toggle(self.view);
                self.proc_scroll = 0;
                EventResult::Consumed(None)
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.proc_scroll += 1;
                EventResult::Consumed(None)
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.proc_scroll = self.proc_scroll.saturating_sub(1);
                EventResult::Consumed(None)
            }
            KeyCode::PageDown => {
                self.proc_scroll += 10;
                EventResult::Consumed(None)
            }
            KeyCode::PageUp => {
                self.proc_scroll = self.proc_scroll.saturating_sub(10);
                EventResult::Consumed(None)
            }
            KeyCode::Char('g') | KeyCode::Home => {
                self.proc_scroll = 0;
                EventResult::Consumed(None)
            }
            _ => EventResult::Consumed(None),
        }
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        use ratatui::style::Modifier as RMod;
        use ratatui::text::{Line, Span};
        use ratatui::widgets::Tabs;

        // Sample + schedule the next animation frame so widgets keep ticking.
        if self.tick() {
            tokio::spawn(async {
                tokio::time::sleep(REFRESH).await;
                zemacs_event::request_redraw();
            });
        }

        let theme = &ctx.editor.theme;
        surface.clear_with(area, panel_bg(theme));
        if area.width < 24 || area.height < 8 {
            surface.set_stringn(
                area.x,
                area.y,
                "  terminal too small for dashboard",
                area.width as usize,
                theme.get("comment"),
            );
            return;
        }

        let dim = to_rat_style(theme.get("comment"));
        let accent = to_rat_style(theme.get("function")).add_modifier(RMod::BOLD);

        // ── sub-view tab strip + host/uptime on the right ───────────────────────
        let tabrow = Rect::new(area.x, area.y, area.width, 1);
        surface.clear_with(tabrow, theme.get("ui.statusline"));
        let tabs = Tabs::new(vec![Line::from("Overview"), Line::from("Processes")])
            .select(self.view as usize)
            .style(dim)
            .highlight_style(accent.add_modifier(RMod::REVERSED))
            .divider(Span::styled("│", dim));
        render(tabs, Rect::new(area.x + 1, area.y, 26, 1), surface);
        // Record click hit-regions matching the rendered layout: "Overview │ Processes"
        // starting at area.x + 1 (Tabs uses a 1-space divider padding each side).
        self.tab_row = area.y;
        let ov0 = area.x + 1;
        let ov1 = ov0 + "Overview".len() as u16;
        let pr0 = ov1 + 3; // " │ "
        let pr1 = pr0 + "Processes".len() as u16;
        self.tab_hits = vec![(ov0, ov1, View::Overview), (pr0, pr1, View::Processes)];
        let right = format!(
            " {} · up {} ",
            System::host_name().unwrap_or_else(|| "host".into()),
            fmt_dur(System::uptime())
        );
        let rw = right.chars().count() as u16;
        if area.width > rw + 30 {
            render(
                ratatui::widgets::Paragraph::new(Span::styled(right, dim)),
                Rect::new(area.x + area.width - rw, area.y, rw, 1),
                surface,
            );
        }

        let body = Rect::new(area.x, area.y + 1, area.width, area.height - 1);
        match self.view {
            View::Overview => self.render_overview(surface, theme, body),
            View::Processes => self.render_processes(surface, theme, body),
        }
    }

    fn id(&self) -> Option<&'static str> {
        Some("dashboard")
    }
}

impl DashboardPanel {
    fn render_overview(&self, surface: &mut Surface, theme: &zemacs_view::Theme, area: Rect) {
        use ratatui::layout::Constraint;
        use ratatui::style::Modifier as RMod;
        use ratatui::symbols::Marker;
        use ratatui::text::{Line, Span};
        use ratatui::widgets::canvas::{Canvas, Points};
        use ratatui::widgets::{
            Axis, BarChart, Chart, Dataset, Gauge, GraphType, LineGauge, List, ListItem, Paragraph,
            Row, Table,
        };

        let dim = to_rat_style(theme.get("comment"));
        let text = to_rat_style(theme.get("ui.text"));
        let accent = to_rat_style(theme.get("function")).add_modifier(RMod::BOLD);
        let good = to_rat_style(theme.get("diff.plus"));
        let warn = to_rat_style(theme.get("diff.delta"));
        let key = to_rat_style(theme.get("keyword"));

        // Three vertical bands.
        let bands = split(area, true, &[7, 9, 8]);
        let (top, mid, bot) = (bands[0], bands[1], bands[2]);

        // ── TOP BAND: CPU gauge+sparkline | Memory line-gauges | This process ───
        let topc = split(top, false, &[1, 1, 1]);

        // CPU card: Gauge + Sparkline
        {
            let inner = card(surface, theme, topc[0], "CPU");
            if inner.height >= 1 {
                let cpu = self.cpu_hist.back().copied().unwrap_or(0.0);
                let gauge = Gauge::default()
                    .ratio((cpu / 100.0).clamp(0.0, 1.0))
                    .label(Span::styled(format!("{cpu:.0}%"), text))
                    .gauge_style(gauge_color(cpu, theme))
                    .use_unicode(true);
                render(gauge, Rect::new(inner.x, inner.y, inner.width, 1), surface);
            }
            if inner.height >= 3 {
                let data: Vec<u64> = self.cpu_hist.iter().map(|v| *v as u64).collect();
                let spark = ratatui::widgets::Sparkline::default()
                    .data(&data)
                    .style(accent);
                render(
                    spark,
                    Rect::new(inner.x, inner.y + 2, inner.width, inner.height - 2),
                    surface,
                );
            }
        }

        // Memory card: LineGauge (mem) + LineGauge (swap) + numbers
        {
            let inner = card(surface, theme, topc[1], "Memory");
            let total = self.sys.total_memory();
            let used = self.sys.used_memory();
            let mem_ratio = if total > 0 {
                used as f64 / total as f64
            } else {
                0.0
            };
            let stot = self.sys.total_swap();
            let sused = self.sys.used_swap();
            let swap_ratio = if stot > 0 {
                sused as f64 / stot as f64
            } else {
                0.0
            };
            if inner.height >= 1 {
                let lg = LineGauge::default()
                    .ratio(mem_ratio.clamp(0.0, 1.0))
                    .filled_style(good)
                    .unfilled_style(dim)
                    .label(Span::styled("RAM", text))
                    .line_set(ratatui::symbols::line::THICK);
                render(lg, Rect::new(inner.x, inner.y, inner.width, 1), surface);
            }
            if inner.height >= 2 {
                let lg = LineGauge::default()
                    .ratio(swap_ratio.clamp(0.0, 1.0))
                    .filled_style(warn)
                    .unfilled_style(dim)
                    .label(Span::styled("swp", text))
                    .line_set(ratatui::symbols::line::THICK);
                render(lg, Rect::new(inner.x, inner.y + 1, inner.width, 1), surface);
            }
            if inner.height >= 4 {
                let lines = vec![
                    Line::from(vec![
                        Span::styled("used ", dim),
                        Span::styled(human(used), text),
                        Span::styled(" / ", dim),
                        Span::styled(human(total), text),
                    ]),
                    Line::from(vec![
                        Span::styled("swap ", dim),
                        Span::styled(human(sused), text),
                        Span::styled(" / ", dim),
                        Span::styled(human(stot), text),
                    ]),
                ];
                render(
                    Paragraph::new(lines),
                    Rect::new(inner.x, inner.y + 3, inner.width, inner.height - 3),
                    surface,
                );
            }
        }

        // This-process card: Table of zemacs' own stats
        {
            let inner = card(surface, theme, topc[2], "zemacs process");
            let proc = self.sys.process(Pid::from_u32(self.pid));
            let (cpu, mem, virt, rt, threads) = match proc {
                Some(p) => (
                    p.cpu_usage(),
                    p.memory(),
                    p.virtual_memory(),
                    p.run_time(),
                    p.tasks().map(|t| t.len()),
                ),
                None => (0.0, 0, 0, 0, None),
            };
            let kv = |k: &'static str, v: String| {
                Row::new(vec![
                    ratatui::widgets::Cell::from(Span::styled(k, dim)),
                    ratatui::widgets::Cell::from(Span::styled(v, text)),
                ])
            };
            let rows = vec![
                kv("pid", self.pid.to_string()),
                kv("cpu", format!("{cpu:.1}%")),
                kv("rss", human(mem)),
                kv("virt", human(virt)),
                kv(
                    "threads",
                    threads.map(|t| t.to_string()).unwrap_or_else(|| "—".into()),
                ),
                kv("uptime", fmt_dur(rt)),
            ];
            let table = Table::new(rows, [Constraint::Length(8), Constraint::Min(6)]).style(text);
            render(table, inner, surface);
        }

        // ── MID BAND: CPU/mem Chart | per-core BarChart | net Canvas ────────────
        let midc = split(mid, false, &[2, 1, 1]);

        // CPU + mem history Chart
        {
            let inner = card(surface, theme, midc[0], "CPU / mem history");
            let cpu: Vec<(f64, f64)> = self
                .cpu_hist
                .iter()
                .enumerate()
                .map(|(i, v)| (i as f64, *v))
                .collect();
            let mem: Vec<(f64, f64)> = self
                .mem_hist
                .iter()
                .enumerate()
                .map(|(i, v)| (i as f64, *v))
                .collect();
            let n = self.cpu_hist.len().max(self.mem_hist.len()).max(2) as f64 - 1.0;
            let datasets = vec![
                Dataset::default()
                    .name("cpu%")
                    .marker(Marker::Braille)
                    .graph_type(GraphType::Line)
                    .style(accent)
                    .data(&cpu),
                Dataset::default()
                    .name("mem%")
                    .marker(Marker::Braille)
                    .graph_type(GraphType::Line)
                    .style(good)
                    .data(&mem),
            ];
            let chart = Chart::new(datasets)
                .x_axis(Axis::default().style(dim).bounds([0.0, n]))
                .y_axis(Axis::default().style(dim).bounds([0.0, 100.0]).labels(vec![
                    Line::from("0"),
                    Line::from("50"),
                    Line::from("100"),
                ]));
            render(chart, inner, surface);
        }

        // Per-core BarChart
        {
            let inner = card(surface, theme, midc[1], "Cores");
            let labels: Vec<String> = (0..self.cores.len()).map(|i| format!("{i}")).collect();
            let data: Vec<(&str, u64)> = self
                .cores
                .iter()
                .enumerate()
                .map(|(i, v)| (labels[i].as_str(), *v as u64))
                .collect();
            if !data.is_empty() && inner.width > 2 {
                let bar_w = ((inner.width as usize / data.len().max(1)).saturating_sub(1))
                    .clamp(1, 4) as u16;
                let chart = BarChart::default()
                    .data(data.as_slice())
                    .bar_width(bar_w.max(1))
                    .bar_gap(1)
                    .max(100)
                    .bar_style(accent)
                    .value_style(dim)
                    .label_style(dim);
                render(chart, inner, surface);
            }
        }

        // Network rate Canvas (rx green / tx orange)
        {
            let inner = card(surface, theme, midc[2], "Net (rx/tx)");
            let max = self
                .net_rx_hist
                .iter()
                .chain(self.net_tx_hist.iter())
                .copied()
                .max()
                .unwrap_or(1)
                .max(1) as f64;
            let n = self.net_rx_hist.len().max(2) as f64 - 1.0;
            let rx: Vec<(f64, f64)> = self
                .net_rx_hist
                .iter()
                .enumerate()
                .map(|(i, v)| (i as f64, *v as f64 / max * 100.0))
                .collect();
            let tx: Vec<(f64, f64)> = self
                .net_tx_hist
                .iter()
                .enumerate()
                .map(|(i, v)| (i as f64, *v as f64 / max * 100.0))
                .collect();
            let rx_color = good.fg.unwrap_or(ratatui::style::Color::Green);
            let tx_color = warn.fg.unwrap_or(ratatui::style::Color::Yellow);
            let bg = crate::ui::rat::to_rat_color(panel_bg_color(theme));
            let canvas = Canvas::default()
                .marker(Marker::Braille)
                .background_color(bg)
                .x_bounds([0.0, n.max(1.0)])
                .y_bounds([0.0, 100.0])
                .paint(move |c| {
                    c.draw(&Points {
                        coords: &rx,
                        color: rx_color,
                    });
                    c.draw(&Points {
                        coords: &tx,
                        color: tx_color,
                    });
                });
            render(canvas, inner, surface);
        }

        // ── BOTTOM BAND: System | Networks list | Disks bars | Calendar ─────────
        let botc = split(bot, false, &[3, 3, 3, 3]);

        // System info Paragraph
        {
            let inner = card(surface, theme, botc[0], "System");
            let load = System::load_average();
            let lines = vec![
                kv_line(
                    "os",
                    System::long_os_version().unwrap_or_default(),
                    dim,
                    text,
                ),
                kv_line(
                    "kernel",
                    System::kernel_version().unwrap_or_default(),
                    dim,
                    text,
                ),
                kv_line("arch", System::cpu_arch(), dim, text),
                kv_line("cpus", self.cores.len().to_string(), dim, text),
                kv_line(
                    "load",
                    format!("{:.2} {:.2} {:.2}", load.one, load.five, load.fifteen),
                    dim,
                    text,
                ),
            ];
            render(Paragraph::new(lines), inner, surface);
        }

        // Networks List
        {
            let inner = card(surface, theme, botc[1], "Interfaces");
            let items: Vec<ListItem> = self
                .nets
                .iter()
                .map(|(name, d)| {
                    ListItem::new(Line::from(vec![
                        Span::styled(format!("{name} "), key),
                        Span::styled(format!("↓{}/s ", human(d.received())), good),
                        Span::styled(format!("↑{}/s", human(d.transmitted())), warn),
                    ]))
                })
                .collect();
            render(List::new(items).style(text), inner, surface);
        }

        // Disks BarChart (used %)
        {
            let inner = card(surface, theme, botc[2], "Disks %used");
            let labels: Vec<String> = self
                .disks
                .iter()
                .map(|d| short_disk(&d.name().to_string_lossy()))
                .collect();
            let data: Vec<(&str, u64)> = self
                .disks
                .iter()
                .enumerate()
                .map(|(i, d)| {
                    let total = d.total_space().max(1);
                    let used = total.saturating_sub(d.available_space());
                    (labels[i].as_str(), used * 100 / total)
                })
                .collect();
            if !data.is_empty() {
                let chart = BarChart::default()
                    .data(data.as_slice())
                    .bar_width(6)
                    .bar_gap(1)
                    .max(100)
                    .bar_style(warn)
                    .value_style(dim)
                    .label_style(dim);
                render(chart, inner, surface);
            }
        }

        // Calendar (current month)
        {
            let inner = card(surface, theme, botc[3], "Calendar");
            render_calendar(surface, theme, inner);
        }
    }

    fn render_processes(&self, surface: &mut Surface, theme: &zemacs_view::Theme, area: Rect) {
        use ratatui::layout::Constraint;
        use ratatui::style::Modifier as RMod;
        use ratatui::text::Span;
        use ratatui::widgets::{Cell, Row, Scrollbar, ScrollbarOrientation, ScrollbarState, Table};

        let dim = to_rat_style(theme.get("comment"));
        let text = to_rat_style(theme.get("ui.text"));
        let accent = to_rat_style(theme.get("function")).add_modifier(RMod::BOLD);

        let inner = card(surface, theme, area, "Top processes (by CPU)");
        if inner.height < 2 {
            return;
        }
        let visible = inner.height.saturating_sub(1) as usize; // minus header row
        let max_scroll = self.procs.len().saturating_sub(visible);
        let scroll = self.proc_scroll.min(max_scroll);

        let rows: Vec<Row> = self
            .procs
            .iter()
            .skip(scroll)
            .take(visible)
            .map(|p| {
                Row::new(vec![
                    Cell::from(Span::styled(p.pid.to_string(), dim)),
                    Cell::from(Span::styled(p.name.clone(), text)),
                    Cell::from(Span::styled(
                        format!("{:.1}%", p.cpu),
                        gauge_color(p.cpu as f64, theme),
                    )),
                    Cell::from(Span::styled(human(p.mem), text)),
                ])
            })
            .collect();
        let widths = [
            Constraint::Length(8),
            Constraint::Min(12),
            Constraint::Length(8),
            Constraint::Length(12),
        ];
        let table = Table::new(rows, widths)
            .header(
                Row::new(vec!["PID", "NAME", "CPU", "MEM"])
                    .style(accent)
                    .bottom_margin(0),
            )
            .column_spacing(1)
            .style(text);
        // leave the last column for the scrollbar
        let table_rect = Rect::new(
            inner.x,
            inner.y,
            inner.width.saturating_sub(1),
            inner.height,
        );
        render(table, table_rect, surface);

        if self.procs.len() > visible {
            let mut sb = ScrollbarState::new(self.procs.len())
                .viewport_content_length(visible)
                .position(scroll);
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("▲"))
                .end_symbol(Some("▼"))
                .track_symbol(Some("│"))
                .thumb_symbol("█")
                .style(dim);
            render_stateful(
                scrollbar,
                Rect::new(inner.x + inner.width - 1, inner.y + 1, 1, inner.height - 1),
                surface,
                &mut sb,
            );
        }
    }
}

// ── helpers ─────────────────────────────────────────────────────────────────

fn push_f(v: &mut VecDeque<f64>, x: f64) {
    v.push_back(x);
    while v.len() > HIST {
        v.pop_front();
    }
}

fn push_u(v: &mut VecDeque<u64>, x: u64) {
    v.push_back(x);
    while v.len() > HIST {
        v.pop_front();
    }
}

/// An opaque background colour for the page. A transparent terminal + a theme
/// whose `ui.background` has no colour would otherwise show the desktop straight
/// through the dashboard's empty cells; fall back through the popup/menu scopes
/// to a dark default so the page reads as a solid panel.
fn panel_bg_color(theme: &zemacs_view::Theme) -> zemacs_view::graphics::Color {
    theme
        .get("ui.background")
        .bg
        .or(theme.get("ui.menu").bg)
        .or(theme.get("ui.popup").bg)
        .or(theme.get("ui.window").bg)
        .unwrap_or(zemacs_view::graphics::Color::Rgb(0x16, 0x18, 0x1e))
}

/// The opaque page background as a style.
fn panel_bg(theme: &zemacs_view::Theme) -> zemacs_view::graphics::Style {
    zemacs_view::graphics::Style::default().bg(panel_bg_color(theme))
}

/// Draw a titled bordered card and return its inner content rect.
fn card(surface: &mut Surface, theme: &zemacs_view::Theme, area: Rect, title: &str) -> Rect {
    use ratatui::style::Modifier as RMod;
    use ratatui::text::Span;
    use ratatui::widgets::{Block, Borders};

    if area.width < 3 || area.height < 3 {
        return area;
    }
    let dim = to_rat_style(theme.get("comment"));
    let accent = to_rat_style(theme.get("function")).add_modifier(RMod::BOLD);
    let bg = panel_bg(theme);
    surface.clear_with(area, bg);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(dim)
        .title(Span::styled(format!(" {title} "), accent))
        .style(to_rat_style(bg));
    render(block, area, surface);
    Rect::new(area.x + 1, area.y + 1, area.width - 2, area.height - 2)
}

/// Split `area` into weighted sub-rects (vertical = stacked rows).
fn split(area: Rect, vertical: bool, weights: &[u16]) -> Vec<Rect> {
    let total: u16 = weights.iter().sum::<u16>().max(1);
    let span = if vertical { area.height } else { area.width };
    let mut out = Vec::with_capacity(weights.len());
    let mut used = 0u16;
    for (i, w) in weights.iter().enumerate() {
        let size = if i + 1 == weights.len() {
            span.saturating_sub(used)
        } else {
            span * w / total
        };
        let r = if vertical {
            Rect::new(area.x, area.y + used, area.width, size)
        } else {
            Rect::new(area.x + used, area.y, size, area.height)
        };
        used = used.saturating_add(size);
        out.push(r);
    }
    out
}

/// A `key: value` ratatui `Line` for the System paragraph.
fn kv_line(
    k: &'static str,
    v: String,
    dim: ratatui::style::Style,
    text: ratatui::style::Style,
) -> ratatui::text::Line<'static> {
    use ratatui::text::{Line, Span};
    Line::from(vec![
        Span::styled(format!("{k:<7}"), dim),
        Span::styled(v, text),
    ])
}

/// Colour a gauge/percentage by load: green < 60, yellow < 85, red beyond.
fn gauge_color(pct: f64, theme: &zemacs_view::Theme) -> ratatui::style::Style {
    let scope = if pct >= 85.0 {
        "error"
    } else if pct >= 60.0 {
        "diff.delta"
    } else {
        "diff.plus"
    };
    to_rat_style(theme.get(scope))
}

/// Render a month calendar for the current (local) month with today marked.
/// Hand-rolled (7×N grid) rather than ratatui's `Monthly`, whose day cells don't
/// render reliably here — the headers showed but the day numbers were blank.
fn render_calendar(surface: &mut Surface, theme: &zemacs_view::Theme, area: Rect) {
    use zemacs_view::graphics::Modifier;

    if area.width < 20 || area.height < 3 {
        return;
    }
    let dim = theme.get("comment");
    let text = theme.get("ui.text");
    let accent = theme.get("function").add_modifier(Modifier::BOLD);
    let today_style = theme.get("function").add_modifier(Modifier::REVERSED);

    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    let today = now.date();
    let (year, month, today_day) = (today.year(), today.month(), today.day());

    // Centered "Month Year" header.
    let title = format!("{month} {year}");
    let tx = area.x + (area.width.saturating_sub(title.chars().count() as u16)) / 2;
    surface.set_stringn(tx, area.y, &title, area.width as usize, accent);

    // Weekday header (Sunday-first), 3 cells wide each to match the day grid.
    surface.set_stringn(
        area.x,
        area.y + 1,
        "Su Mo Tu We Th Fr Sa",
        area.width as usize,
        dim,
    );

    let first = match time::Date::from_calendar_date(year, month, 1) {
        Ok(d) => d,
        Err(_) => return,
    };
    let start_col = first.weekday().number_days_from_sunday() as u16; // 0 = Sunday
    let days = month.length(year);

    let mut col = start_col;
    let mut row = area.y + 2;
    for day in 1..=days {
        if row >= area.y + area.height {
            break;
        }
        let x = area.x + col * 3;
        let style = if day == today_day { today_style } else { text };
        surface.set_stringn(x, row, &format!("{day:>2}"), 2, style);
        col += 1;
        if col == 7 {
            col = 0;
            row += 1;
        }
    }
}

/// Human-readable byte size (binary units).
fn human(bytes: u64) -> String {
    const U: [&str; 6] = ["B", "K", "M", "G", "T", "P"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes}{}", U[0])
    } else {
        format!("{v:.1}{}", U[i])
    }
}

/// `123s` → `2m 3s`, etc.
fn fmt_dur(secs: u64) -> String {
    let (d, h, m, s) = (secs / 86400, secs / 3600 % 24, secs / 60 % 60, secs % 60);
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    }
}

/// Shorten a disk/device name to its last path component for a bar label.
fn short_disk(name: &str) -> String {
    name.rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(name)
        .chars()
        .take(6)
        .collect()
}
