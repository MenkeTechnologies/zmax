//! Dissociate — the zemacs port of GNU Emacs `dissociated-press`
//! (play/dissociate.el): a travesty generator that scrambles the current
//! buffer's text amusingly.
//!
//! The algorithm (char-continuity mode, the default `arg` = 2): copy a short run
//! of source text to the output, take the last `arg` characters as an
//! "overlap", jump to a random point in the source, search forward (wrapping)
//! for that overlap, and continue copying from just after it. Emacs redisplays
//! the `*Dissociation*` buffer as text is added and asks whether to continue
//! every screenful; this overlay generates a bounded block up front (pure,
//! unit-tested) and reveals it progressively, closing on any key.

use std::time::{Duration, Instant};

use tui::buffer::Buffer as Surface;
use zemacs_view::graphics::Rect;

use crate::compositor::{Callback, Component, Compositor, Context, Event, EventResult};

/// Total characters of travesty to generate for one run.
const OUTPUT_CHARS: usize = 3000;

fn rand(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *state >> 33
}

/// Return the index just past the first occurrence of `needle` in `hay` at or
/// after `from` (searching only up to `limit`), like emacs `search-forward`.
fn search_forward(hay: &[char], needle: &[char], from: usize, limit: usize) -> Option<usize> {
    if needle.is_empty() || needle.len() > hay.len() {
        return None;
    }
    let last = limit.min(hay.len()).saturating_sub(needle.len());
    let mut i = from.min(hay.len());
    while i <= last {
        if hay[i..i + needle.len()] == *needle {
            return Some(i + needle.len());
        }
        i += 1;
    }
    None
}

/// Generate up to `max_chars` of dissociated text from `source`, using `arg`
/// characters of continuity. Faithful to `dissociated-press`'s char mode.
pub fn dissociate(source: &[char], arg: usize, max_chars: usize, rng: &mut u64) -> String {
    let n = source.len();
    if n == 0 || arg == 0 {
        return String::new();
    }
    let mut out: Vec<char> = Vec::new();
    let mut point = 0usize;
    while out.len() < max_chars {
        let start = point;
        // end = start + arg + random(16); if past the end, restart near the top.
        let mut end = start + arg + (rand(rng) as usize % 16);
        if end > n {
            end = arg + (rand(rng) as usize % 16);
        }
        end = end.min(n);
        // insert-buffer-substring copies the region between start and end in
        // buffer order regardless of which is larger.
        let (a, b) = (start.min(end), start.max(end));
        out.extend_from_slice(&source[a..b]);
        point = end;
        // Overlap continuity: find the last `arg` chars elsewhere and resume.
        if point >= n {
            point = 0;
            continue;
        }
        let ov_start = point.saturating_sub(arg);
        let overlap = &source[ov_start..point];
        let jump = rand(rng) as usize % n;
        point = search_forward(source, overlap, jump, n)
            .or_else(|| search_forward(source, overlap, 0, jump))
            .unwrap_or(0);
    }
    out.truncate(max_chars);
    out.into_iter().collect()
}

/// The `dissociated-press` overlay.
pub struct Dissociate {
    text: Vec<char>,
    revealed: usize,
    last: Option<Instant>,
    interval: Duration,
}

impl Dissociate {
    /// Build the travesty from `source` text (the current buffer), continuity
    /// `arg` (emacs default 2), seeded by `seed`.
    pub fn new(source: &str, arg: usize, seed: u64) -> Self {
        let chars: Vec<char> = source.chars().collect();
        let mut rng = seed | 1;
        let text = dissociate(&chars, arg, OUTPUT_CHARS, &mut rng)
            .chars()
            .collect();
        Dissociate {
            text,
            revealed: 0,
            last: None,
            interval: Duration::from_millis(40),
        }
    }
}

impl Component for Dissociate {
    fn handle_event(&mut self, event: &Event, _cx: &mut Context) -> EventResult {
        if let Event::Key(_) = event {
            let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
                compositor.pop();
            });
            return EventResult::Consumed(Some(close));
        }
        EventResult::Ignored(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        if self.revealed < self.text.len() {
            let now = Instant::now();
            let due = match self.last {
                Some(t) => now.duration_since(t) >= self.interval,
                None => true,
            };
            if due {
                self.last = Some(now);
                // Reveal roughly a line's worth per tick.
                self.revealed = (self.revealed + area.width as usize).min(self.text.len());
            }
            zemacs_event::request_redraw();
        }

        let theme = &ctx.editor.theme;
        let bg = theme.get("ui.background");
        let text_style = theme.get("ui.text");
        surface.clear_with(area, bg);

        // Word-wrap the revealed prefix into the frame, keeping the tail visible.
        let width = area.width.max(1) as usize;
        let mut lines: Vec<String> = Vec::new();
        let mut cur = String::new();
        for &ch in &self.text[..self.revealed] {
            if ch == '\n' {
                lines.push(std::mem::take(&mut cur));
            } else {
                cur.push(ch);
                if cur.chars().count() >= width {
                    lines.push(std::mem::take(&mut cur));
                }
            }
        }
        lines.push(cur);
        let rows = area.height as usize;
        let start = lines.len().saturating_sub(rows);
        for (row, line) in lines[start..].iter().enumerate() {
            surface.set_string(area.x, area.y + row as u16, line, text_style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_forward_finds_match_end() {
        let hay: Vec<char> = "abcabc".chars().collect();
        let needle: Vec<char> = "bc".chars().collect();
        assert_eq!(search_forward(&hay, &needle, 0, hay.len()), Some(3));
        // From index 2 the next "bc" ends at 6.
        assert_eq!(search_forward(&hay, &needle, 2, hay.len()), Some(6));
        // Limited search can't reach the second occurrence.
        assert_eq!(search_forward(&hay, &needle, 2, 4), None);
    }

    #[test]
    fn output_is_drawn_only_from_source_characters() {
        let source: Vec<char> = "the quick brown fox jumps over the lazy dog".chars().collect();
        let allowed: std::collections::HashSet<char> = source.iter().copied().collect();
        let mut rng = 12345u64;
        let out = dissociate(&source, 2, 500, &mut rng);
        assert!(!out.is_empty());
        assert!(out.chars().all(|c| allowed.contains(&c)), "no alien chars");
    }

    #[test]
    fn empty_or_zero_arg_yields_empty() {
        let mut rng = 1u64;
        assert_eq!(dissociate(&[], 2, 100, &mut rng), "");
        let src: Vec<char> = "abc".chars().collect();
        assert_eq!(dissociate(&src, 0, 100, &mut rng), "");
    }

    #[test]
    fn generation_is_deterministic_for_a_fixed_seed() {
        let src: Vec<char> = "aabbccddeeff".chars().collect();
        let out1 = dissociate(&src, 2, 200, &mut 999u64);
        let out2 = dissociate(&src, 2, 200, &mut 999u64);
        assert_eq!(out1, out2);
        assert_eq!(out1.chars().count(), 200);
    }
}
