//! ANSI projection for finalized transcript lines.

use std::io::{self, Write};

use crate::render_runtime::common::style::{StyledLine, TextColor, TextStyle};

pub(crate) fn write_ansi_line(output: &mut dyn Write, line: &StyledLine) -> io::Result<()> {
    for segment in &line.segments {
        write_style(output, segment.style)?;
        output.write_all(segment.text.as_bytes())?;
        reset_style(output)?;
    }
    output.write_all(b"\n")
}

fn write_style(output: &mut dyn Write, style: TextStyle) -> io::Result<()> {
    if style == TextStyle::default() {
        return Ok(());
    }
    output.write_all(b"\x1b[")?;
    let mut first = true;
    if style.bold {
        output.write_all(b"1")?;
        first = false;
    }
    write_color(output, style.foreground, &mut first)?;
    output.write_all(b"m")
}

fn write_color(output: &mut dyn Write, color: TextColor, first: &mut bool) -> io::Result<()> {
    let Some(code) = color_code(color) else {
        return Ok(());
    };
    if !*first {
        output.write_all(b";")?;
    }
    output.write_all(code.as_bytes())?;
    *first = false;
    Ok(())
}

fn color_code(color: TextColor) -> Option<String> {
    match color {
        TextColor::Default => None,
        TextColor::Green => Some("32".to_string()),
    }
}

fn reset_style(output: &mut dyn Write) -> io::Result<()> {
    output.write_all(b"\x1b[0m")
}

#[cfg(test)]
mod tests {
    use super::write_ansi_line;
    use crate::render_runtime::common::style::{StyledLine, StyledSegment, TextColor, TextStyle};

    #[test]
    fn writes_renderer_neutral_style_as_ansi() {
        let line = StyledLine::from_segments(vec![StyledSegment::new(
            "ok",
            TextStyle::foreground(TextColor::Green).bold(),
        )]);
        let mut output = Vec::new();

        write_ansi_line(&mut output, &line).unwrap();

        assert_eq!(
            String::from_utf8(output).unwrap(),
            "\u{1b}[1;32mok\u{1b}[0m\n"
        );
    }
}
