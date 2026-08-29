//! Primitives for styled text.
//!
//! A terminal UI is at its root a lot of strings. In order to make it accessible and stylish,
//! those strings may be associated to a set of styles. `tui` has three ways to represent them:
//! - A single line string where all graphemes have the same style is represented by a [`Span`].
//! - A single line string where each grapheme may have its own style is represented by [`Spans`].
//! - A multiple line string where each grapheme may have its own style is represented by a
//!   [`Text`].
//!
//! These types form a hierarchy: [`Spans`] is a collection of [`Span`] and each line of [`Text`]
//! is a [`Spans`].
//!
//! Keep in mind that a lot of widgets will use those types to advertise what kind of string is
//! supported for their properties. Moreover, `tui` provides convenient `From` implementations so
//! that you can start by using simple `String` or `&str` and then promote them to the previous
//! primitives when you need additional styling capabilities.
//!
//! For example, for the [`crate::widgets::Block`] widget, all the following calls are valid to set
//! its `title` property (which is a [`Spans`] under the hood):
//!
//! ```rust
//! # use zmax_tui::widgets::Block;
//! # use zmax_tui::text::{Span, Spans};
//! # use zmax_view::graphics::{Color, Style};
//! // A simple string with no styling.
//! // Converted to Spans(vec![
//! //   Span { content: Cow::Borrowed("My title"), style: Style { .. } }
//! // ])
//! let block = Block::default().title("My title");
//!
//! // A simple string with a unique style.
//! // Converted to Spans(vec![
//! //   Span { content: Cow::Borrowed("My title"), style: Style { fg: Some(Color::Yellow), .. }
//! // ])
//! let block = Block::default().title(
//!     Span::styled("My title", Style::default().fg(Color::Yellow))
//! );
//!
//! // A string with multiple styles.
//! // Converted to Spans(vec![
//! //   Span { content: Cow::Borrowed("My"), style: Style { fg: Some(Color::Yellow), .. } },
//! //   Span { content: Cow::Borrowed(" title"), .. }
//! // ])
//! let block = Block::default().title(vec![
//!     Span::styled("My", Style::default().fg(Color::Yellow)),
//!     Span::raw(" title"),
//! ]);
//! ```
use std::borrow::Cow;
use unicode_segmentation::UnicodeSegmentation;
use zmax_core::line_ending::str_is_line_ending;
use zmax_core::unicode::width::UnicodeWidthStr;
use zmax_view::graphics::Style;

/// A grapheme associated to a style.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyledGrapheme<'a> {
    pub symbol: &'a str,
    pub style: Style,
}

/// A string where all graphemes have the same style.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span<'a> {
    pub content: Cow<'a, str>,
    pub style: Style,
}

impl<'a> Span<'a> {
    /// Create a span with no style.
    ///
    /// ## Examples
    ///
    /// ```rust
    /// # use zmax_tui::text::Span;
    /// Span::raw("My text");
    /// Span::raw(String::from("My text"));
    /// ```
    pub fn raw<T>(content: T) -> Span<'a>
    where
        T: Into<Cow<'a, str>>,
    {
        Span {
            content: content.into(),
            style: Style::default(),
        }
    }

    /// Create a span with a style.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use zmax_tui::text::Span;
    /// # use zmax_view::graphics::{Color, Modifier, Style};
    /// let style = Style::default().fg(Color::Yellow).add_modifier(Modifier::ITALIC);
    /// Span::styled("My text", style);
    /// Span::styled(String::from("My text"), style);
    /// ```
    pub fn styled<T>(content: T, style: Style) -> Span<'a>
    where
        T: Into<Cow<'a, str>>,
    {
        Span {
            content: content.into(),
            style,
        }
    }

    /// Returns the width of the content held by this span.
    pub fn width(&self) -> usize {
        self.content.width()
    }

    /// Returns an iterator over the graphemes held by this span.
    ///
    /// `base_style` is the [`Style`] that will be patched with each grapheme [`Style`] to get
    /// the resulting [`Style`].
    ///
    /// ## Examples
    ///
    /// ```rust
    /// # use zmax_tui::text::{Span, StyledGrapheme};
    /// # use zmax_view::graphics::{Color, Modifier, Style};
    /// # use std::iter::Iterator;
    /// let style = Style::default().fg(Color::Yellow);
    /// let span = Span::styled("Text", style);
    /// let style = Style::default().fg(Color::Green).bg(Color::Black);
    /// let styled_graphemes = span.styled_graphemes(style);
    /// assert_eq!(
    ///     vec![
    ///         StyledGrapheme {
    ///             symbol: "T",
    ///             style: Style {
    ///                 fg: Some(Color::Yellow),
    ///                 bg: Some(Color::Black),
    ///                 underline_color: None,
    ///                 underline_style: None,
    ///                 add_modifier: Modifier::empty(),
    ///                 sub_modifier: Modifier::empty(),
    ///             },
    ///         },
    ///         StyledGrapheme {
    ///             symbol: "e",
    ///             style: Style {
    ///                 fg: Some(Color::Yellow),
    ///                 bg: Some(Color::Black),
    ///                 underline_color: None,
    ///                 underline_style: None,
    ///                 add_modifier: Modifier::empty(),
    ///                 sub_modifier: Modifier::empty(),
    ///             },
    ///         },
    ///         StyledGrapheme {
    ///             symbol: "x",
    ///             style: Style {
    ///                 fg: Some(Color::Yellow),
    ///                 bg: Some(Color::Black),
    ///                 underline_color: None,
    ///                 underline_style: None,
    ///                 add_modifier: Modifier::empty(),
    ///                 sub_modifier: Modifier::empty(),
    ///             },
    ///         },
    ///         StyledGrapheme {
    ///             symbol: "t",
    ///             style: Style {
    ///                 fg: Some(Color::Yellow),
    ///                 bg: Some(Color::Black),
    ///                 underline_color: None,
    ///                 underline_style: None,
    ///                 add_modifier: Modifier::empty(),
    ///                 sub_modifier: Modifier::empty(),
    ///             },
    ///         },
    ///     ],
    ///     styled_graphemes.collect::<Vec<StyledGrapheme>>()
    /// );
    /// ```
    pub fn styled_graphemes(
        &'a self,
        base_style: Style,
    ) -> impl Iterator<Item = StyledGrapheme<'a>> {
        UnicodeSegmentation::graphemes(self.content.as_ref(), true)
            .map(move |g| StyledGrapheme {
                symbol: g,
                style: base_style.patch(self.style),
            })
            .filter(|s| !str_is_line_ending(s.symbol))
    }
}

impl<'a> From<String> for Span<'a> {
    fn from(s: String) -> Span<'a> {
        Span::raw(s)
    }
}

impl<'a> From<&'a str> for Span<'a> {
    fn from(s: &'a str) -> Span<'a> {
        Span::raw(s)
    }
}

impl<'a> From<Cow<'a, str>> for Span<'a> {
    fn from(s: Cow<'a, str>) -> Span<'a> {
        Span::raw(s)
    }
}

/// A string composed of clusters of graphemes, each with their own style.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Spans<'a>(pub Vec<Span<'a>>);

impl Spans<'_> {
    /// Returns the width of the underlying string.
    ///
    /// ## Examples
    ///
    /// ```rust
    /// # use zmax_tui::text::{Span, Spans};
    /// # use zmax_view::graphics::{Color, Style};
    /// let spans = Spans::from(vec![
    ///     Span::styled("My", Style::default().fg(Color::Yellow)),
    ///     Span::raw(" text"),
    /// ]);
    /// assert_eq!(7, spans.width());
    /// ```
    pub fn width(&self) -> usize {
        self.0.iter().map(Span::width).sum()
    }
}

impl<'a> From<String> for Spans<'a> {
    fn from(s: String) -> Spans<'a> {
        Spans(vec![Span::from(s)])
    }
}

impl<'a> From<&'a str> for Spans<'a> {
    fn from(s: &'a str) -> Spans<'a> {
        Spans(vec![Span::from(s)])
    }
}

impl<'a> From<Cow<'a, str>> for Spans<'a> {
    fn from(s: Cow<'a, str>) -> Spans<'a> {
        Spans(vec![Span::raw(s)])
    }
}

impl<'a> From<Vec<Span<'a>>> for Spans<'a> {
    fn from(spans: Vec<Span<'a>>) -> Spans<'a> {
        Spans(spans)
    }
}

impl<'a> From<Span<'a>> for Spans<'a> {
    fn from(span: Span<'a>) -> Spans<'a> {
        Spans(vec![span])
    }
}

impl<'a> From<Spans<'a>> for String {
    fn from(line: Spans<'a>) -> String {
        line.0.iter().map(|s| &*s.content).collect()
    }
}

impl<'a> From<&Spans<'a>> for String {
    fn from(line: &Spans<'a>) -> String {
        line.0.iter().map(|s| &*s.content).collect()
    }
}

/// A string split over multiple lines where each line is composed of several clusters, each with
/// their own style.
///
/// A [`Text`], like a [`Span`], can be constructed using one of the many `From` implementations
/// or via the [`Text::raw`] and [`Text::styled`] methods. Helpfully, [`Text`] also implements
/// [`core::iter::Extend`] which enables the concatenation of several [`Text`] blocks.
///
/// ```rust
/// # use zmax_tui::text::Text;
/// # use zmax_view::graphics::{Color, Modifier, Style};
/// let style = Style::default().fg(Color::Yellow).add_modifier(Modifier::ITALIC);
///
/// // An initial two lines of `Text` built from a `&str`
/// let mut text = Text::from("The first line\nThe second line");
/// assert_eq!(2, text.height());
///
/// // Adding two more unstyled lines
/// text.extend(Text::raw("These are two\nmore lines!"));
/// assert_eq!(4, text.height());
///
/// // Adding a final two styled lines
/// text.extend(Text::styled("Some more lines\nnow with more style!", style));
/// assert_eq!(6, text.height());
/// ```
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Text<'a> {
    pub lines: Vec<Spans<'a>>,
}

impl<'a> Text<'a> {
    /// Create some text (potentially multiple lines) with no style.
    ///
    /// ## Examples
    ///
    /// ```rust
    /// # use zmax_tui::text::Text;
    /// Text::raw("The first line\nThe second line");
    /// Text::raw(String::from("The first line\nThe second line"));
    /// ```
    pub fn raw<T>(content: T) -> Text<'a>
    where
        T: Into<Cow<'a, str>>,
    {
        Text {
            lines: match content.into() {
                Cow::Borrowed(s) => s.lines().map(Spans::from).collect(),
                Cow::Owned(s) => s.lines().map(|l| Spans::from(l.to_owned())).collect(),
            },
        }
    }

    /// Create some text (potentially multiple lines) with a style.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use zmax_tui::text::Text;
    /// # use zmax_view::graphics::{Color, Modifier, Style};
    /// let style = Style::default().fg(Color::Yellow).add_modifier(Modifier::ITALIC);
    /// Text::styled("The first line\nThe second line", style);
    /// Text::styled(String::from("The first line\nThe second line"), style);
    /// ```
    pub fn styled<T>(content: T, style: Style) -> Text<'a>
    where
        T: Into<Cow<'a, str>>,
    {
        let mut text = Text::raw(content);
        text.patch_style(style);
        text
    }

    /// Returns the max width of all the lines.
    ///
    /// ## Examples
    ///
    /// ```rust
    /// use zmax_tui::text::Text;
    /// let text = Text::from("The first line\nThe second line");
    /// assert_eq!(15, text.width());
    /// ```
    pub fn width(&self) -> usize {
        self.lines
            .iter()
            .map(Spans::width)
            .max()
            .unwrap_or_default()
    }

    /// Returns the height.
    ///
    /// ## Examples
    ///
    /// ```rust
    /// use zmax_tui::text::Text;
    /// let text = Text::from("The first line\nThe second line");
    /// assert_eq!(2, text.height());
    /// ```
    pub fn height(&self) -> usize {
        self.lines.len()
    }

    /// Patch text with a new style. Only updates fields that are in the new style.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use zmax_tui::text::Text;
    /// # use zmax_view::graphics::{Color,  Style};
    /// let style1 = Style::default().fg(Color::Yellow);
    /// let style2 = Style::default().fg(Color::Yellow).bg(Color::Black);
    /// let mut half_styled_text = Text::styled(String::from("The first line\nThe second line"), style1);
    /// let full_styled_text = Text::styled(String::from("The first line\nThe second line"), style2);
    /// assert_ne!(half_styled_text, full_styled_text);
    ///
    /// half_styled_text.patch_style(Style::default().bg(Color::Black));
    /// assert_eq!(half_styled_text, full_styled_text);
    /// ```
    pub fn patch_style(&mut self, style: Style) {
        for line in &mut self.lines {
            for span in &mut line.0 {
                span.style = span.style.patch(style);
            }
        }
    }

    /// Apply a new style to existing text.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use zmax_tui::text::Text;
    /// # use zmax_view::graphics::{Color, Modifier, Style};
    /// let style = Style::default().fg(Color::Yellow).add_modifier(Modifier::ITALIC);
    /// let mut raw_text = Text::raw("The first line\nThe second line");
    /// let styled_text = Text::styled(String::from("The first line\nThe second line"), style);
    /// assert_ne!(raw_text, styled_text);
    ///
    /// raw_text.set_style(style);
    /// assert_eq!(raw_text, styled_text);
    /// ```
    pub fn set_style(&mut self, style: Style) {
        for line in &mut self.lines {
            for span in &mut line.0 {
                span.style = style;
            }
        }
    }
}

impl<'a> From<String> for Text<'a> {
    fn from(s: String) -> Text<'a> {
        Text::raw(s)
    }
}

impl<'a> From<&'a str> for Text<'a> {
    fn from(s: &'a str) -> Text<'a> {
        Text::raw(s)
    }
}

impl<'a> From<Cow<'a, str>> for Text<'a> {
    fn from(s: Cow<'a, str>) -> Text<'a> {
        Text::raw(s)
    }
}

impl<'a> From<Span<'a>> for Text<'a> {
    fn from(span: Span<'a>) -> Text<'a> {
        Text {
            lines: vec![Spans::from(span)],
        }
    }
}

impl<'a> From<Spans<'a>> for Text<'a> {
    fn from(spans: Spans<'a>) -> Text<'a> {
        Text { lines: vec![spans] }
    }
}

impl<'a> From<Vec<Spans<'a>>> for Text<'a> {
    fn from(lines: Vec<Spans<'a>>) -> Text<'a> {
        Text { lines }
    }
}

impl<'a> From<Text<'a>> for String {
    fn from(text: Text<'a>) -> String {
        String::from(&text)
    }
}

impl<'a> From<&Text<'a>> for String {
    fn from(text: &Text<'a>) -> String {
        let size = text
            .lines
            .iter()
            .flat_map(|spans| spans.0.iter().map(|span| span.content.len()))
            .sum::<usize>()
            + text.lines.len().saturating_sub(1); // for newline after each line
        let mut output = String::with_capacity(size);

        for spans in &text.lines {
            if !output.is_empty() {
                output.push('\n');
            }
            for span in &spans.0 {
                output.push_str(&span.content);
            }
        }
        output
    }
}

impl<'a> IntoIterator for Text<'a> {
    type Item = Spans<'a>;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.lines.into_iter()
    }
}

impl<'a> Extend<Spans<'a>> for Text<'a> {
    fn extend<T: IntoIterator<Item = Spans<'a>>>(&mut self, iter: T) {
        self.lines.extend(iter);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zmax_view::graphics::{Color, Modifier};

    /// Width is display columns, not characters: a CJK glyph occupies two cells
    /// and a combining mark none. Every layout decision -- centring, truncation,
    /// the statusline -- is made from this number, so counting characters here
    /// would misalign the whole frame.
    #[test]
    fn width_counts_display_columns_not_characters() {
        assert_eq!(Span::raw("hello").width(), 5);
        assert_eq!(Span::raw("日本語").width(), 6, "three double-width glyphs");
        // `e` plus a combining acute: one column, two chars.
        assert_eq!(Span::raw("e\u{0301}").width(), 1);
        assert_eq!(Span::raw("").width(), 0);

        // A line's width is the sum of its spans...
        let spans = Spans::from(vec![Span::raw("日本"), Span::raw("ab")]);
        assert_eq!(spans.width(), 6);
        // ...and a block's is its widest line, with height the line count.
        let text = Text::from("ab\n日本語\nc");
        assert_eq!(text.width(), 6);
        assert_eq!(text.height(), 3);
    }

    /// `Text::raw` splits on `str::lines`, so a trailing newline does not add an
    /// empty last line and `\r\n` is one break rather than two. Getting this
    /// wrong shows a spurious blank row under popups and hover text.
    #[test]
    fn lines_split_without_a_trailing_empty_row() {
        assert_eq!(Text::from("a\nb").height(), 2);
        assert_eq!(Text::from("a\nb\n").height(), 2, "no empty last line");
        assert_eq!(Text::from("a\r\nb").height(), 2, "CRLF is one break");
        assert_eq!(Text::from("a\n\nb").height(), 3, "a real blank line stays");
        assert_eq!(Text::from("").height(), 0);
        assert_eq!(Text::from("").width(), 0);
    }

    /// The grapheme iterator yields clusters, not chars, and silently drops line
    /// endings -- a renderer that emitted them would move the cursor mid-row.
    #[test]
    fn styled_graphemes_yields_clusters_and_drops_line_endings() {
        // The iterator borrows the span it came from, so the clusters are copied
        // out before the span goes out of scope.
        let symbols = |content: &str| -> Vec<String> {
            let span = Span::raw(content.to_string());
            span.styled_graphemes(Style::default())
                .map(|g| g.symbol.to_string())
                .collect()
        };

        // A combining mark stays attached to its base character.
        assert_eq!(symbols("e\u{0301}x"), vec!["e\u{0301}", "x"]);
        assert_eq!(symbols("日本"), vec!["日", "本"]);
        // Line endings are filtered out, including CRLF as a single cluster.
        assert_eq!(symbols("a\nb"), vec!["a", "b"]);
        assert_eq!(symbols("a\r\nb"), vec!["a", "b"]);
        assert_eq!(symbols("\n"), Vec::<String>::new());
    }

    /// A grapheme's style is the base patched with the span's, so the span only
    /// overrides what it actually sets and inherits the rest.
    #[test]
    fn a_graphemes_style_is_the_base_patched_with_the_spans() {
        let base = Style::default().fg(Color::Green).bg(Color::Black);
        let span = Span::styled("x", Style::default().fg(Color::Yellow));

        let grapheme = span.styled_graphemes(base).next().unwrap();

        assert_eq!(grapheme.style.fg, Some(Color::Yellow), "the span wins");
        assert_eq!(grapheme.style.bg, Some(Color::Black), "the base shows through");
    }

    /// `patch_style` merges into what a span already carries; `set_style`
    /// replaces it. Confusing the two either loses syntax highlighting or fails
    /// to apply a selection.
    #[test]
    fn patch_style_merges_where_set_style_replaces() {
        let italic = Style::default().add_modifier(Modifier::ITALIC);
        let yellow = Style::default().fg(Color::Yellow);

        let mut patched = Text::styled("a\nb", yellow);
        patched.patch_style(italic);
        for line in &patched.lines {
            for span in &line.0 {
                assert_eq!(span.style.fg, Some(Color::Yellow), "kept");
                assert!(span.style.add_modifier.contains(Modifier::ITALIC), "added");
            }
        }

        let mut replaced = Text::styled("a\nb", yellow);
        replaced.set_style(italic);
        for line in &replaced.lines {
            for span in &line.0 {
                assert_eq!(span.style.fg, None, "the colour is gone");
                assert!(span.style.add_modifier.contains(Modifier::ITALIC));
            }
        }
    }
}
