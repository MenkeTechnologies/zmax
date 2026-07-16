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

/// Emacs `fill-region-as-paragraph`: treat the whole region as ONE paragraph —
/// blank lines and existing line breaks inside it are not paragraph boundaries,
/// they are folded away — then fill it to `text_width`. This is what separates
/// it from `fill-region` (which fills each paragraph in the region separately,
/// keeping the blank lines between them).
///
/// The indentation of the first non-blank line is reused as the paragraph's
/// indent, matching emacs's `fill-prefix`-less default.
pub fn fill_as_paragraph(text: &str, text_width: usize) -> SmartString<LazyCompact> {
    let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let lead: String = first
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect();
    let body = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if body.is_empty() {
        return SmartString::new();
    }
    let options = Options::new(text_width)
        .word_splitter(NoHyphenation)
        .word_separator(textwrap::WordSeparator::AsciiSpace)
        .initial_indent(&lead)
        .subsequent_indent(&lead);
    textwrap::fill(&body, options).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `fill-region-as-paragraph` folds blank lines away: the region becomes a
    /// single paragraph, unlike `fill-region`, which would keep the break.
    #[test]
    fn fill_as_paragraph_joins_across_blank_lines() {
        let out = fill_as_paragraph("alpha beta\n\ngamma delta\n", 40);
        assert_eq!(out.as_str(), "alpha beta gamma delta");
    }

    /// The first line's indent becomes the paragraph indent for every line, and
    /// the text is wrapped at the fill column.
    #[test]
    fn fill_as_paragraph_keeps_indent_and_wraps() {
        let out = fill_as_paragraph("  one two three four five six", 12);
        assert_eq!(out.as_str(), "  one two\n  three four\n  five six");
    }

    /// A whitespace-only region fills to nothing rather than to a blank line.
    #[test]
    fn fill_as_paragraph_empty() {
        assert_eq!(fill_as_paragraph("  \n\n \n", 40).as_str(), "");
    }

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
