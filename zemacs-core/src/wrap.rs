use smartstring::{LazyCompact, SmartString};
use textwrap::{Options, WordSplitter::NoHyphenation};

/// Given a slice of text, return the text re-wrapped to fit it
/// within the given width.
pub fn reflow_hard_wrap(text: &str, text_width: usize) -> SmartString<LazyCompact> {
    let options = Options::new(text_width)
        .word_splitter(NoHyphenation)
        .word_separator(textwrap::WordSeparator::AsciiSpace);
    textwrap::refill(text, options).into()
}

/// vim `formatoptions+=n` with `formatlistpat`: reflow a numbered/bulleted list
/// item so its continuation lines line up under the text, not under the marker.
/// `hang` is the visual width of the marker (`1. `, `- `, …) the wrapped lines
/// are indented by.
pub fn reflow_hanging(text: &str, text_width: usize, hang: usize) -> SmartString<LazyCompact> {
    // `refill` reuses the first line's indent for *every* line, which is exactly
    // what a hanging indent must not do — so unfill the paragraph by hand and
    // fill it with the two indents spelled out.
    let lead: String = text
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect();
    let body = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let subsequent = format!("{lead}{}", " ".repeat(hang));
    let options = Options::new(text_width)
        .word_splitter(NoHyphenation)
        .word_separator(textwrap::WordSeparator::AsciiSpace)
        .initial_indent(&lead)
        .subsequent_indent(&subsequent);
    textwrap::fill(&body, options).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A wrapped list item must not put its continuation flush with the marker —
    /// that is the whole difference `formatoptions+=n` makes.
    #[test]
    fn reflow_hanging_indents_the_continuation_under_the_text() {
        let out = reflow_hanging("1. alpha beta gamma delta epsilon", 16, 3);
        assert_eq!(out.as_str(), "1. alpha beta\n   gamma delta\n   epsilon");
        // Zero hang is plain refill, same as `reflow_hard_wrap`.
        assert_eq!(
            reflow_hanging("alpha beta gamma", 11, 0).as_str(),
            reflow_hard_wrap("alpha beta gamma", 11).as_str()
        );
    }
}
