//! Example plugin: prose statistics for the current buffer.
//!
//! Demonstrates [`Host::word_count`], which returns chars, words and lines in
//! ONE call. Counting them separately would mean three passes over the buffer
//! and three chances to disagree with the editor's own definition of a word —
//! this is the editor's count, not a reimplementation of it.
//!
//! [`Host::indent`] supplies the other half: indentation in COLUMNS, with a tab
//! counting as `tabstop` rather than as one character. For prose that is what
//! separates a block quote from a paragraph.
//!
//! ```text
//! :plugin load .../libzmax_native_writing_stats.dylib
//! :writing   # → "820 words in 6 paragraphs · avg 18 words/sentence · 2 indented blocks"
//! ```

use std::os::raw::c_int;

use zmax_native::{declare_plugin, Args, Host};

/// A paragraph break is a blank line; everything else continues the paragraph.
///
/// Counted from the line texts rather than from the word count, since the
/// editor has no notion of a paragraph.
fn paragraph_count(lines: &[String]) -> usize {
    let mut paragraphs = 0usize;
    let mut in_paragraph = false;
    for line in lines {
        if line.trim().is_empty() {
            in_paragraph = false;
        } else if !in_paragraph {
            paragraphs += 1;
            in_paragraph = true;
        }
    }
    paragraphs
}

/// Sentences, counted by terminal punctuation.
///
/// Deliberately crude — abbreviations inflate it — so the average it feeds is
/// described as approximate rather than presented as exact.
fn sentence_count(text: &str) -> usize {
    text.matches(['.', '!', '?']).count()
}

/// Words per sentence, or `None` when there are no sentences to divide by.
fn words_per_sentence(words: usize, sentences: usize) -> Option<usize> {
    (sentences > 0).then(|| words / sentences)
}

/// Lines indented past the body text, which for prose marks quotes and code
/// blocks. Measured in columns, so a tab-indented line counts by its tab stop.
fn indented_blocks(indents: &[usize]) -> usize {
    let mut blocks = 0usize;
    let mut inside = false;
    for &indent in indents {
        if indent > 0 && !inside {
            blocks += 1;
            inside = true;
        } else if indent == 0 {
            inside = false;
        }
    }
    blocks
}

/// The report line.
fn report(words: usize, paragraphs: usize, per_sentence: Option<usize>, blocks: usize) -> String {
    if words == 0 {
        return "nothing written yet".to_string();
    }
    let pace = match per_sentence {
        Some(n) => format!(" · avg ~{n} words/sentence"),
        None => String::new(),
    };
    let quoted = if blocks > 0 {
        format!(" · {blocks} indented blocks")
    } else {
        String::new()
    };
    format!("{words} words in {paragraphs} paragraphs{pace}{quoted}")
}

/// `:writing` — prose statistics for the buffer.
fn writing(host: &Host, _args: &Args) -> c_int {
    // One call for all three counts, using the editor's own definition of a
    // word rather than a second one written here.
    let Some((_chars, words, _lines)) = host.word_count() else {
        host.error("writing: no active buffer");
        return 1;
    };
    let Some(text) = host.buffer_text() else {
        host.error("writing: no active buffer");
        return 1;
    };

    let count = host.line_count();
    let lines = host.lines(0, count);
    let indents: Vec<usize> = (0..count).map(|line| host.indent(line)).collect();

    host.message(&report(
        words,
        paragraph_count(&lines),
        words_per_sentence(words, sentence_count(&text)),
        indented_blocks(&indents),
    ));
    0
}

declare_plugin! {
    name: "writing-stats",
    version: "0.1.0",
    commands: { "writing" => writing },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(raw: &[&str]) -> Vec<String> {
        raw.iter().map(|s| s.to_string()).collect()
    }

    /// Consecutive non-blank lines are one paragraph; a blank line ends it.
    #[test]
    fn blank_lines_separate_paragraphs() {
        assert_eq!(paragraph_count(&lines(&["a", "b", "", "c"])), 2);
        assert_eq!(paragraph_count(&lines(&["a", "b", "c"])), 1);
        assert_eq!(paragraph_count(&lines(&[])), 0);
    }

    /// Several blank lines in a row still separate exactly one boundary, and
    /// leading blanks do not open an empty paragraph.
    #[test]
    fn runs_of_blank_lines_do_not_multiply() {
        assert_eq!(paragraph_count(&lines(&["a", "", "", "", "b"])), 2);
        assert_eq!(paragraph_count(&lines(&["", "", "a"])), 1);
        assert_eq!(paragraph_count(&lines(&["", ""])), 0, "blank only");
    }

    /// Dividing by sentences is skipped rather than dividing by zero when the
    /// text has no terminal punctuation.
    #[test]
    fn no_sentences_means_no_average() {
        assert_eq!(words_per_sentence(50, 0), None);
        assert_eq!(words_per_sentence(50, 5), Some(10));
    }

    /// An indented run is one block however many lines it spans, and returning
    /// to column 0 ends it.
    #[test]
    fn an_indented_run_is_one_block() {
        assert_eq!(indented_blocks(&[0, 4, 4, 4, 0]), 1);
        assert_eq!(indented_blocks(&[0, 4, 0, 4, 0]), 2, "two separate blocks");
        assert_eq!(indented_blocks(&[0, 0, 0]), 0);
    }

    /// The average is marked approximate, because counting sentences by
    /// punctuation is fooled by abbreviations.
    #[test]
    fn the_average_is_marked_approximate() {
        let line = report(820, 6, Some(18), 2);
        assert!(line.contains("~18 words/sentence"), "the ~ is deliberate");
        assert!(line.contains("820 words in 6 paragraphs"));
        assert!(line.contains("2 indented blocks"));
    }

    /// Absent facts are omitted rather than shown as zero, and an empty buffer
    /// is stated outright.
    #[test]
    fn absent_facts_are_omitted() {
        let plain = report(100, 2, None, 0);
        assert!(!plain.contains("words/sentence"));
        assert!(!plain.contains("indented"));
        assert_eq!(report(0, 0, None, 0), "nothing written yet");
    }
}
