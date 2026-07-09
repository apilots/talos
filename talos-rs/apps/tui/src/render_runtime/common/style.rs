//! Renderer-neutral text styling used by both Ratatui and ANSI projections.

/// Terminal color that can be projected to Ratatui or ANSI output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextColor {
    Default,
    Green,
}

/// Renderer-neutral style for a transcript text segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TextStyle {
    pub(crate) foreground: TextColor,
    pub(crate) bold: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            foreground: TextColor::Default,
            bold: false,
        }
    }
}

impl TextStyle {
    pub(crate) fn foreground(foreground: TextColor) -> Self {
        Self {
            foreground,
            ..Self::default()
        }
    }

    pub(crate) fn bold(mut self) -> Self {
        self.bold = true;
        self
    }
}

/// A single styled text run in a transcript line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StyledSegment {
    pub(crate) text: String,
    pub(crate) style: TextStyle,
}

impl StyledSegment {
    pub(crate) fn new(text: impl Into<String>, style: TextStyle) -> Self {
        Self {
            text: text.into(),
            style,
        }
    }

    pub(crate) fn plain(text: impl Into<String>) -> Self {
        Self::new(text, TextStyle::default())
    }
}

/// A renderer-neutral transcript line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StyledLine {
    pub(crate) segments: Vec<StyledSegment>,
}

impl StyledLine {
    pub(crate) fn from_segments(segments: Vec<StyledSegment>) -> Self {
        Self { segments }
    }

    pub(crate) fn plain(text: impl Into<String>) -> Self {
        Self {
            segments: vec![StyledSegment::plain(text)],
        }
    }

    pub(crate) fn plain_lines(text: &str) -> Vec<Self> {
        text.trim()
            .lines()
            .map(|line| Self::plain(line.to_string()))
            .collect()
    }

    pub(crate) fn plain_text(&self) -> String {
        self.segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect()
    }
}
