//! Doctor — the zmax port of GNU Emacs `doctor`, the ELIZA psychotherapist.
//!
//! A conversational overlay: type a line, press `RET`, and the doctor replies
//! with a reflected/keyword-driven response in the ELIZA tradition. `q` at an
//! empty prompt or `C-c` quits. The response engine (keyword rules + pronoun
//! reflection + rotating fallbacks) is pure and unit-tested (keys parse into a
//! `doctor` keymap mode via `scripts/gen_port_report.py`).

use tui::buffer::Buffer as Surface;
use zmax_view::graphics::Rect;

use crate::{
    compositor::{Callback, Component, Compositor, Context, Event, EventResult},
    ctrl, key,
};

/// Swap first/second-person pronouns so the doctor can echo the patient's words
/// back ("I am" -> "you are", "my" -> "your", …).
fn reflect(input: &str) -> String {
    input
        .split_whitespace()
        .map(|w| {
            let lower = w.to_ascii_lowercase();
            match lower.as_str() {
                "i" => "you",
                "me" => "you",
                "my" => "your",
                "mine" => "yours",
                "am" => "are",
                "i'm" => "you're",
                "myself" => "yourself",
                "you" => "I",
                "your" => "my",
                "yours" => "mine",
                "yourself" => "myself",
                _ => w,
            }
            .to_string()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Produce the doctor's reply to `input`. `turn` rotates the fallback lines so
/// repeated non-keyword remarks do not always get the same answer. Pure.
pub fn respond(input: &str, turn: usize) -> String {
    let text = input.trim();
    if text.is_empty() {
        return "Please, go on.".into();
    }
    let lower = text.to_ascii_lowercase();
    let strip = |kw: &str| -> Option<String> {
        lower
            .find(kw)
            .map(|i| reflect(text[i + kw.len()..].trim_end_matches(['.', '!', '?', ' '])))
    };

    if let Some(rest) = strip("i need ") {
        return format!("Why do you need {rest}?");
    }
    if let Some(rest) = strip("i am ").or_else(|| strip("i'm ")) {
        return format!("How long have you been {rest}?");
    }
    if let Some(rest) = strip("i want ") {
        return format!("What would it mean to you if you got {rest}?");
    }
    if let Some(rest) = strip("i feel ") {
        return format!("Tell me more about feeling {rest}.");
    }
    if lower.contains("because") {
        return "Is that the real reason?".into();
    }
    if lower.contains("mother") || lower.contains("father") || lower.contains("family") {
        return "Tell me more about your family.".into();
    }
    if lower.contains("dream") {
        return "What does that dream suggest to you?".into();
    }
    if lower.contains("computer") || lower.contains("emacs") {
        return "Do machines worry you?".into();
    }
    if lower.contains("sorry") {
        return "Please don't apologize.".into();
    }
    if lower.starts_with("yes") {
        return "You seem quite certain.".into();
    }
    if lower.starts_with("no") {
        return "Why not?".into();
    }
    if lower.contains('?') {
        return "Why do you ask that?".into();
    }

    const FALLBACKS: &[&str] = &[
        "Can you elaborate on that?",
        "Why do you say that?",
        "I see. Please continue.",
        "How does that make you feel?",
        "What else comes to mind?",
        "Does talking about this bother you?",
    ];
    FALLBACKS[turn % FALLBACKS.len()].to_string()
}

/// A line of the transcript.
struct Line {
    from_patient: bool,
    text: String,
}

/// The interactive Doctor overlay.
pub struct Doctor {
    log: Vec<Line>,
    input: String,
    turn: usize,
}

impl Doctor {
    pub fn new() -> Self {
        Doctor {
            log: vec![Line {
                from_patient: false,
                text: "I am the psychotherapist. Please, describe your problems.".into(),
            }],
            input: String::new(),
            turn: 0,
        }
    }

    fn submit(&mut self) {
        let said = self.input.trim().to_string();
        if said.is_empty() {
            return;
        }
        let reply = respond(&said, self.turn);
        self.turn += 1;
        self.log.push(Line {
            from_patient: true,
            text: said,
        });
        self.log.push(Line {
            from_patient: false,
            text: reply,
        });
        self.input.clear();
    }
}

impl Default for Doctor {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Doctor {
    fn handle_event(&mut self, event: &Event, _cx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => *key,
            _ => return EventResult::Ignored(None),
        };
        let close: Callback = Box::new(|compositor: &mut Compositor, _cx| {
            compositor.pop();
        });
        match key {
            ctrl!('c') | key!(Esc) => return EventResult::Consumed(Some(close)),
            key!('q') if self.input.is_empty() => return EventResult::Consumed(Some(close)),
            key!(Enter) => self.submit(),
            key!(Backspace) => {
                self.input.pop();
            }
            key!(c @ ' '..='~') => self.input.push(c),
            _ => {}
        }
        EventResult::Consumed(None)
    }

    fn render(&mut self, area: Rect, surface: &mut Surface, ctx: &mut Context) {
        let theme = &ctx.editor.theme;
        let mut bg = theme.get("ui.background");
        // `transparent-background`: drop the panel fill so the terminal shows
        // through, matching the editor surface and the rest of the IDE.
        if ctx.editor.config().transparent_background {
            bg.bg = None;
        }
        let text_style = theme.get("ui.text");
        let doc_style = theme.get("ui.text.focus");
        let you_style = theme.get("ui.linenr");
        let prompt_style = theme.get("ui.selection");

        surface.clear_with(area, bg);
        if area.width < 20 || area.height < 6 {
            return;
        }
        let ox = area.x + 1;
        let width = area.width.saturating_sub(2) as usize;
        let input_y = area.y + area.height - 1;

        // Show the tail of the transcript that fits above the input line.
        let visible = (area.height as usize).saturating_sub(2);
        let start = self.log.len().saturating_sub(visible);
        for (i, line) in self.log[start..].iter().enumerate() {
            let (tag, style) = if line.from_patient {
                ("You: ", you_style)
            } else {
                ("Dr:  ", doc_style)
            };
            let mut s = format!("{tag}{}", line.text);
            s.truncate(width);
            surface.set_string(ox, area.y + i as u16, &s, style);
        }

        let mut prompt = format!("> {}", self.input);
        prompt.truncate(width);
        surface.set_string(ox, input_y, &prompt, prompt_style);
        let _ = text_style;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reflect_swaps_pronouns() {
        assert_eq!(reflect("i am my own"), "you are your own");
        assert_eq!(reflect("you are yours"), "I are mine");
    }

    #[test]
    fn keyword_rules_fire() {
        assert!(respond("I need a vacation", 0).starts_with("Why do you need"));
        assert!(respond("I am sad", 0).starts_with("How long have you been"));
        assert_eq!(
            respond("Tell me about my mother", 0),
            "Tell me more about your family."
        );
        assert_eq!(respond("no", 0), "Why not?");
        assert_eq!(respond("What is emacs?", 0), "Do machines worry you?");
    }

    #[test]
    fn i_need_reflects_the_object() {
        assert_eq!(respond("I need you", 0), "Why do you need I?");
    }

    #[test]
    fn fallbacks_rotate_across_turns() {
        let a = respond("the sky is blue", 0);
        let b = respond("the sky is blue", 1);
        assert_ne!(a, b, "consecutive fallbacks should differ");
    }

    #[test]
    fn empty_input_is_gently_prompted() {
        assert_eq!(respond("   ", 3), "Please, go on.");
    }
}
