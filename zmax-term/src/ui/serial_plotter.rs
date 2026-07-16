//! Serial Plotter — the zmax port of the Arduino IDE / PlatformIO serial
//! plotter.
//!
//! A [`Component`] modal panel that runs the board's serial monitor
//! (`arduino-cli monitor` or `pio device monitor`) as a child process, reads its
//! stdout line by line in a background thread, parses each line into numeric
//! series (the Arduino plotter wire format — whitespace/comma separated numbers,
//! optionally `label:value`), and draws a live [`braille`]-rasterised line chart.
//!
//! Keys: `q`/`Esc` close, `c` clears the history, `p` pauses/resumes ingest.
//!
//! Open: `:arduino-plotter` / `:serial-plotter`.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tui::buffer::Buffer as Surface;
use tui::symbols::braille;
use zmax_view::{
    graphics::{Color, CursorKind, Rect, Style},
    input::KeyCode,
};

use crate::compositor::{Component, Compositor, Context, Event, EventResult};

/// How many samples per series to retain (the plot window scrolls as new
/// samples arrive).
const HISTORY: usize = 2000;

/// A palette for distinguishing up to six plotted series.
const SERIES_COLORS: [Color; 6] = [
    Color::Cyan,
    Color::Yellow,
    Color::Magenta,
    Color::Green,
    Color::Red,
    Color::Blue,
];

/// One named data series (unlabelled channels get `ch0`, `ch1`, …).
#[derive(Debug, Clone, Default)]
pub struct Series {
    pub label: String,
    pub data: VecDeque<f32>,
}

/// The shared, thread-safe plot model the reader thread fills and `render` reads.
#[derive(Debug, Default)]
pub struct PlotData {
    pub series: Vec<Series>,
    /// Total lines ingested (shown in the status bar).
    pub samples: u64,
    pub paused: bool,
}

impl PlotData {
    /// Ingest one parsed serial line (a set of channel values) into the series,
    /// creating channels as needed and trimming each to [`HISTORY`].
    pub fn push_line(&mut self, values: &[(Option<String>, f32)]) {
        if values.is_empty() {
            return;
        }
        self.samples += 1;
        for (i, (label, value)) in values.iter().enumerate() {
            if i >= self.series.len() {
                self.series.push(Series {
                    label: label.clone().unwrap_or_else(|| format!("ch{i}")),
                    data: VecDeque::with_capacity(HISTORY),
                });
            }
            if let Some(l) = label {
                // A later labelled line can name a previously-anonymous channel.
                if self.series[i].label != *l {
                    self.series[i].label = l.clone();
                }
            }
            let s = &mut self.series[i];
            s.data.push_back(*value);
            while s.data.len() > HISTORY {
                s.data.pop_front();
            }
        }
    }

    /// Min/max across every series' data, for the vertical scale. Falls back to
    /// `(0, 1)` when there's no data, and pads a flat line so it doesn't collapse.
    pub fn bounds(&self) -> (f32, f32) {
        let mut min = f32::INFINITY;
        let mut max = f32::NEG_INFINITY;
        for s in &self.series {
            for &v in &s.data {
                if v.is_finite() {
                    min = min.min(v);
                    max = max.max(v);
                }
            }
        }
        if !min.is_finite() || !max.is_finite() {
            return (0.0, 1.0);
        }
        if (max - min).abs() < f32::EPSILON {
            return (min - 1.0, max + 1.0);
        }
        (min, max)
    }

    pub fn clear(&mut self) {
        self.series.clear();
        self.samples = 0;
    }
}

/// Parse one serial line into `(optional label, value)` channels. Accepts the
/// Arduino plotter formats: bare numbers separated by space / tab / comma, and
/// `label:value` (or `label=value`) pairs. Non-numeric tokens are skipped so a
/// stray log line doesn't poison the plot.
pub fn parse_serial_line(line: &str) -> Vec<(Option<String>, f32)> {
    let line = line.trim();
    if line.is_empty() {
        return Vec::new();
    }
    line.split([',', ' ', '\t'])
        .filter(|t| !t.is_empty())
        .filter_map(|tok| {
            if let Some((label, val)) = tok.split_once([':', '=']) {
                let v: f32 = val.trim().parse().ok()?;
                let label = label.trim();
                let label = (!label.is_empty()).then(|| label.to_string());
                Some((label, v))
            } else {
                let v: f32 = tok.parse().ok()?;
                Some((None, v))
            }
        })
        .collect()
}

/// Map a value to a braille sub-row (`0` = top) within `dot_rows` total rows.
fn dot_row(value: f32, min: f32, max: f32, dot_rows: usize) -> usize {
    if dot_rows == 0 || (max - min).abs() < f32::EPSILON {
        return dot_rows.saturating_sub(1) / 2;
    }
    let norm = ((value - min) / (max - min)).clamp(0.0, 1.0);
    // Invert: higher value → higher on screen → smaller row index.
    let row = ((1.0 - norm) * (dot_rows.saturating_sub(1)) as f32).round() as usize;
    row.min(dot_rows.saturating_sub(1))
}

/// The sample a given plot dot-column maps to: the plot shows the most recent
/// `dot_cols` samples, right-aligned to the newest.
fn sample_at(data: &VecDeque<f32>, dx: usize, dot_cols: usize) -> Option<f32> {
    let len = data.len();
    if len == 0 || dot_cols == 0 {
        return None;
    }
    if len >= dot_cols {
        // Newest window: dx=0 is the oldest visible sample.
        data.get(len - dot_cols + dx).copied()
    } else {
        // Fewer samples than columns: left-align, leave the right empty.
        data.get(dx).copied()
    }
}

pub struct SerialPlotter {
    data: Arc<Mutex<PlotData>>,
    child: std::process::Child,
    dead: Arc<AtomicBool>,
    title: String,
}

impl SerialPlotter {
    /// Spawn `argv` (a serial-monitor command) and start plotting its output.
    pub fn spawn(argv: &[String], title: impl Into<String>) -> std::io::Result<Self> {
        let mut child = std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .stdin(std::process::Stdio::null())
            .spawn()?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| std::io::Error::other("no stdout from serial monitor"))?;

        let data = Arc::new(Mutex::new(PlotData::default()));
        let dead = Arc::new(AtomicBool::new(false));
        {
            let data = data.clone();
            let dead = dead.clone();
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stdout);
                let mut buf = String::new();
                loop {
                    buf.clear();
                    match reader.read_line(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            let values = parse_serial_line(&buf);
                            if let Ok(mut d) = data.lock() {
                                if !d.paused {
                                    d.push_line(&values);
                                }
                            }
                            zmax_event::request_redraw();
                        }
                    }
                }
                dead.store(true, Ordering::Relaxed);
                zmax_event::request_redraw();
            });
        }

        Ok(Self {
            data,
            child,
            dead,
            title: title.into(),
        })
    }

    /// A snapshot of ingest progress: `(lines ingested, channel count)`. Lets a
    /// caller (status line, test) observe the live reader without touching the
    /// internal lock protocol.
    pub fn snapshot(&self) -> (u64, usize) {
        self.data
            .lock()
            .map(|d| (d.samples, d.series.len()))
            .unwrap_or((0, 0))
    }

    fn close() -> EventResult {
        EventResult::Consumed(Some(Box::new(|c: &mut Compositor, _| {
            c.pop();
        })))
    }
}

impl Drop for SerialPlotter {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Component for SerialPlotter {
    fn handle_event(&mut self, event: &Event, _cx: &mut Context) -> EventResult {
        let Event::Key(key) = event else {
            return EventResult::Ignored(None);
        };
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc | KeyCode::F(12) => Self::close(),
            KeyCode::Char('c') => {
                if let Ok(mut d) = self.data.lock() {
                    d.clear();
                }
                EventResult::Consumed(None)
            }
            KeyCode::Char('p') => {
                if let Ok(mut d) = self.data.lock() {
                    d.paused = !d.paused;
                }
                EventResult::Consumed(None)
            }
            // Swallow everything else so the panel is modal.
            _ => EventResult::Consumed(None),
        }
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        surface.clear_with(area, bg);
        let text = theme.get("ui.text");
        let dim = theme.get("ui.text.inactive");

        if area.height < 3 || area.width < 4 {
            return;
        }

        let data = self.data.lock().unwrap();
        let (min, max) = data.bounds();
        let dead = self.dead.load(Ordering::Relaxed);

        // ── Title row ─────────────────────────────────────────────────────────
        let paused = if data.paused { "  [PAUSED]" } else { "" };
        let status = if dead { "  (monitor closed)" } else { "" };
        let title = format!(" {} — {} samples{paused}{status}", self.title, data.samples);
        surface.set_stringn(area.x, area.y, &title, area.width as usize, text);

        // Legend on the title row, right-aligned.
        let mut lx = area.x + area.width;
        for (i, s) in data.series.iter().enumerate().take(SERIES_COLORS.len()) {
            let tag = format!(" {}={:.2} ", s.label, s.data.back().copied().unwrap_or(0.0));
            let w = tag.chars().count() as u16;
            if lx.saturating_sub(w) <= area.x + title.chars().count() as u16 {
                break;
            }
            lx -= w;
            surface.set_stringn(
                lx,
                area.y,
                &tag,
                w as usize,
                Style::default().fg(SERIES_COLORS[i]),
            );
        }

        // ── Plot area (leave a left gutter for y labels, a bottom status row) ──
        let gutter = 8u16.min(area.width / 3);
        let plot = Rect {
            x: area.x + gutter,
            y: area.y + 1,
            width: area.width - gutter,
            height: area.height - 2,
        };

        // Y-axis labels (max at top, min at bottom).
        surface.set_stringn(area.x, plot.y, &format!("{max:>7.2}"), gutter as usize, dim);
        surface.set_stringn(
            area.x,
            plot.y + plot.height.saturating_sub(1),
            &format!("{min:>7.2}"),
            gutter as usize,
            dim,
        );

        if plot.width == 0 || plot.height == 0 {
            return;
        }

        let dot_cols = plot.width as usize * 2;
        let dot_rows = plot.height as usize * 4;

        // Per-cell accumulated braille bits + the first series colour to touch it.
        let cells_w = plot.width as usize;
        let cells_h = plot.height as usize;
        let mut bits = vec![0u16; cells_w * cells_h];
        let mut colors = vec![None::<Color>; cells_w * cells_h];

        for (si, s) in data.series.iter().enumerate() {
            let color = SERIES_COLORS[si % SERIES_COLORS.len()];
            for dx in 0..dot_cols {
                let Some(v) = sample_at(&s.data, dx, dot_cols) else {
                    continue;
                };
                if !v.is_finite() {
                    continue;
                }
                let dy = dot_row(v, min, max, dot_rows);
                let cx = dx / 2;
                let cy = dy / 4;
                if cx >= cells_w || cy >= cells_h {
                    continue;
                }
                let idx = cy * cells_w + cx;
                bits[idx] |= braille::DOTS[dy % 4][dx % 2];
                colors[idx].get_or_insert(color);
            }
        }

        for cy in 0..cells_h {
            for cx in 0..cells_w {
                let idx = cy * cells_w + cx;
                if bits[idx] == 0 {
                    continue;
                }
                let ch = char::from_u32(braille::BLANK as u32 + bits[idx] as u32).unwrap_or(' ');
                let style = Style::default().fg(colors[idx].unwrap_or(Color::White));
                if let Some(cell) = surface.get_mut(plot.x + cx as u16, plot.y + cy as u16) {
                    cell.set_char(ch);
                    cell.set_style(style);
                }
            }
        }

        // ── Bottom help row ───────────────────────────────────────────────────
        let help = " q close · c clear · p pause ";
        surface.set_stringn(
            area.x,
            area.y + area.height - 1,
            help,
            area.width as usize,
            dim,
        );
    }

    fn cursor(
        &self,
        _area: Rect,
        _ctx: &zmax_view::Editor,
    ) -> (Option<zmax_core::Position>, CursorKind) {
        (None, CursorKind::Hidden)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bare_numbers() {
        let v = parse_serial_line("1 2.5 -3");
        assert_eq!(v.len(), 3);
        assert_eq!(v[0], (None, 1.0));
        assert_eq!(v[1], (None, 2.5));
        assert_eq!(v[2], (None, -3.0));
    }

    #[test]
    fn parses_comma_separated() {
        let v = parse_serial_line("10,20,30");
        assert_eq!(
            v.iter().map(|(_, f)| *f).collect::<Vec<_>>(),
            vec![10.0, 20.0, 30.0]
        );
    }

    #[test]
    fn parses_labelled_pairs() {
        let v = parse_serial_line("temp:22.5 hum=60");
        assert_eq!(v[0].0.as_deref(), Some("temp"));
        assert_eq!(v[0].1, 22.5);
        assert_eq!(v[1].0.as_deref(), Some("hum"));
        assert_eq!(v[1].1, 60.0);
    }

    #[test]
    fn skips_non_numeric_tokens() {
        // A stray log line yields no numeric channels.
        assert!(parse_serial_line("Booting sensor...").is_empty());
        // Mixed: only the number survives.
        let v = parse_serial_line("value 42 done");
        assert_eq!(v, vec![(None, 42.0)]);
    }

    #[test]
    fn push_line_creates_and_trims_channels() {
        let mut d = PlotData::default();
        for i in 0..(HISTORY + 50) {
            d.push_line(&[(None, i as f32), (Some("b".into()), -(i as f32))]);
        }
        assert_eq!(d.series.len(), 2);
        assert_eq!(d.series[0].data.len(), HISTORY);
        assert_eq!(d.series[1].label, "b");
        assert_eq!(d.samples as usize, HISTORY + 50);
    }

    #[test]
    fn bounds_spans_all_series() {
        let mut d = PlotData::default();
        d.push_line(&[(None, -5.0), (None, 10.0)]);
        let (lo, hi) = d.bounds();
        assert_eq!(lo, -5.0);
        assert_eq!(hi, 10.0);
    }

    #[test]
    fn bounds_pads_a_flat_line() {
        let mut d = PlotData::default();
        d.push_line(&[(None, 3.0)]);
        d.push_line(&[(None, 3.0)]);
        let (lo, hi) = d.bounds();
        assert!(
            lo < 3.0 && hi > 3.0,
            "flat line must not collapse: {lo}..{hi}"
        );
    }

    #[test]
    fn dot_row_maps_extremes() {
        // Max value → top row (0); min value → bottom row.
        assert_eq!(dot_row(10.0, 0.0, 10.0, 8), 0);
        assert_eq!(dot_row(0.0, 0.0, 10.0, 8), 7);
        // Midpoint lands in the middle-ish.
        let mid = dot_row(5.0, 0.0, 10.0, 8);
        assert!((3..=4).contains(&mid), "mid row was {mid}");
    }

    #[test]
    fn sample_at_right_aligns_full_window() {
        let data: VecDeque<f32> = (0..10).map(|i| i as f32).collect();
        // Window of 4 cols shows the newest 4: [6,7,8,9].
        assert_eq!(sample_at(&data, 0, 4), Some(6.0));
        assert_eq!(sample_at(&data, 3, 4), Some(9.0));
    }
}
