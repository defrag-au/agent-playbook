//! What a terminal screen looked like at one moment, in the two forms a reviewer needs:
//! plain text for the screen itself, and typed runs/warnings for the things text alone loses.

use serde::Serialize;
use vt100::{Color, Screen};

use crate::scenario::Size;

#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub size: Size,
    /// One entry per screen row, right-trimmed. This is exactly what the `.txt` capture holds.
    pub lines: Vec<String>,
    /// The column after the last non-blank cell, parallel to `lines`.
    pub row_cells: Vec<u16>,
    pub cursor: Cursor,
    pub alternate_screen: bool,
    pub styles: Vec<StyleRun>,
    pub styles_truncated: bool,
    /// Rows whose last non-blank cell is the final column. A border legitimately does this; a
    /// sentence cut mid-word does too, and the count is what a glance misses.
    pub edge_rows: Vec<u16>,
    pub warnings: Vec<Warning>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Cursor {
    pub row: u16,
    pub col: u16,
    pub hidden: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct StyleRun {
    pub row: u16,
    /// First column, counted from zero.
    pub start: u16,
    /// Number of cells the run covers.
    pub len: u16,
    /// The run's text, right-trimmed. `start` and `len` carry the true extent.
    pub text: String,
    pub fg: StyleColor,
    pub bg: StyleColor,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StyleColor {
    Default,
    Indexed { index: u8 },
    Rgb { red: u8, green: u8, blue: u8 },
}

impl From<Color> for StyleColor {
    fn from(color: Color) -> Self {
        match color {
            Color::Default => StyleColor::Default,
            Color::Idx(index) => StyleColor::Indexed { index },
            Color::Rgb(red, green, blue) => StyleColor::Rgb { red, green, blue },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Warning {
    WideCharAtEdge {
        row: u16,
    },
    OrphanWideContinuation {
        row: u16,
        col: u16,
    },
    ControlCharacter {
        row: u16,
        col: u16,
        code: u32,
    },
    Bell {
        row: u16,
    },
    BlankScreen,
    /// The screen was still changing when the wait expired. The capture is still useful.
    StillChanging {
        quiet_ms: u64,
    },
    /// A clean exit should have restored the primary screen first.
    ExitedInAlternateScreen {
        code: Option<i32>,
    },
    /// More styled runs existed than the report carries.
    StylesTruncated,
}

impl Snapshot {
    pub fn capture(screen: &Screen) -> Self {
        let (rows, cols) = screen.size();
        let mut lines = Vec::with_capacity(rows as usize);
        let mut row_cells = Vec::with_capacity(rows as usize);
        let mut edge_rows = Vec::new();
        let mut warnings = Vec::new();
        let mut styles = Vec::new();
        let mut styles_truncated = false;

        for row in 0..rows {
            let mut line = String::new();
            let mut last_used = 0;
            let mut run: Option<(u16, CellStyle)> = None;

            for col in 0..cols {
                let Some(cell) = screen.cell(row, col) else {
                    continue;
                };

                if cell.is_wide_continuation() {
                    if col == 0
                        || screen
                            .cell(row, col - 1)
                            .is_none_or(|previous| !previous.is_wide())
                    {
                        warnings.push(Warning::OrphanWideContinuation { row, col });
                    }
                } else {
                    // A cell the app never wrote is still a space on the screen: ratatui sends only
                    // the cells that changed, so the gap between two printed runs is never in the
                    // byte stream. Skipping these ran the words of every capture together.
                    let contents = cell.contents();
                    let text = if contents.is_empty() {
                        " ".to_owned()
                    } else {
                        contents
                    };
                    line.push_str(&text);
                    if text != " " {
                        last_used = if cell.is_wide() {
                            (col + 2).min(cols)
                        } else {
                            col + 1
                        };
                    }
                    for character in text.chars() {
                        if character == '\u{7}' {
                            warnings.push(Warning::Bell { row });
                        } else if character.is_control() && character != '\t' {
                            warnings.push(Warning::ControlCharacter {
                                row,
                                col,
                                code: character as u32,
                            });
                        }
                    }
                }

                let style = CellStyle::from_cell(cell);
                match &run {
                    Some((start, current)) if *current == style => {
                        let _ = start;
                    }
                    Some((start, current)) => {
                        push_run(
                            &mut styles,
                            &mut styles_truncated,
                            screen,
                            row,
                            *start,
                            col,
                            *current,
                        );
                        run = Some((col, style));
                    }
                    None => run = Some((col, style)),
                }

                if col + 1 == cols && cell.is_wide() {
                    warnings.push(Warning::WideCharAtEdge { row });
                }
            }

            if let Some((start, style)) = run {
                push_run(
                    &mut styles,
                    &mut styles_truncated,
                    screen,
                    row,
                    start,
                    cols,
                    style,
                );
            }

            if last_used == cols {
                let edge = match screen.cell(row, cols - 1) {
                    Some(cell) if cell.is_wide() => true,
                    Some(cell) if cell.is_wide_continuation() => false,
                    Some(cell) => edge_content(cell),
                    None => false,
                };
                if edge {
                    edge_rows.push(row);
                }
            }
            row_cells.push(last_used);
            lines.push(line.trim_end().to_owned());
        }

        if lines.iter().all(String::is_empty) {
            warnings.push(Warning::BlankScreen);
        }
        if styles_truncated {
            warnings.push(Warning::StylesTruncated);
        }

        let (cursor_row, cursor_col) = screen.cursor_position();
        Snapshot {
            size: Size { cols, rows },
            lines,
            row_cells,
            cursor: Cursor {
                row: cursor_row,
                col: cursor_col,
                hidden: screen.hide_cursor(),
            },
            alternate_screen: screen.alternate_screen(),
            styles,
            styles_truncated,
            edge_rows,
            warnings,
        }
    }

    pub fn text(&self) -> String {
        let mut text = self.lines.join("\n");
        text.push('\n');
        text
    }

    pub fn contains(&self, needle: &str) -> bool {
        self.lines.iter().any(|line| line.contains(needle))
            || self.lines.join("\n").contains(needle)
    }
}

/// Four hundred styled runs is far more than a review needs and bounds a full-screen gradient.
const MAX_STYLE_RUNS: usize = 400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CellStyle {
    fg: StyleColor,
    bg: StyleColor,
    bold: bool,
    italic: bool,
    underline: bool,
    inverse: bool,
}

impl CellStyle {
    fn from_cell(cell: &vt100::Cell) -> Self {
        Self {
            fg: cell.fgcolor().into(),
            bg: cell.bgcolor().into(),
            bold: cell.bold(),
            italic: cell.italic(),
            underline: cell.underline(),
            inverse: cell.inverse(),
        }
    }

    fn is_default(self) -> bool {
        self == Self {
            fg: StyleColor::Default,
            bg: StyleColor::Default,
            bold: false,
            italic: false,
            underline: false,
            inverse: false,
        }
    }
}

/// A row's final column is worth reporting when the thing there is prose that may have been cut,
/// not chrome that is supposed to reach the edge.
fn edge_content(cell: &vt100::Cell) -> bool {
    let contents = cell.contents();
    let Some(character) = contents.chars().next() else {
        return false;
    };
    if character.is_whitespace() {
        return false;
    }
    !matches!(
        character,
        '\u{2500}'..='\u{257f}' // box drawing
            | '\u{2580}'..='\u{259f}' // block elements
            | '\u{25a0}'..='\u{25ff}' // geometric shapes
    )
}

fn push_run(
    runs: &mut Vec<StyleRun>,
    truncated: &mut bool,
    screen: &Screen,
    row: u16,
    start: u16,
    end: u16,
    style: CellStyle,
) {
    if style.is_default() || end <= start {
        return;
    }
    if runs.len() >= MAX_STYLE_RUNS {
        *truncated = true;
        return;
    }

    let mut text = String::new();
    for col in start..end {
        if let Some(cell) = screen.cell(row, col) {
            if cell.is_wide_continuation() {
                continue;
            }
            let contents = cell.contents();
            if contents.is_empty() {
                text.push(' ');
            } else {
                text.push_str(&contents);
            }
        }
    }

    runs.push(StyleRun {
        row,
        start,
        len: end - start,
        text: text.trim_end().to_owned(),
        fg: style.fg,
        bg: style.bg,
        bold: style.bold,
        italic: style.italic,
        underline: style.underline,
        inverse: style.inverse,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(rows: u16, cols: u16, bytes: &[u8]) -> Snapshot {
        let mut parser = vt100::Parser::new(rows, cols, 10);
        parser.process(bytes);
        Snapshot::capture(parser.screen())
    }

    #[test]
    fn a_cell_the_app_never_wrote_is_a_space() {
        // A renderer sends only the cells that changed, so a gap between two printed runs never
        // appears in the byte stream. The terminal shows its blank grid there, and so must this.
        let snapshot = snapshot(2, 10, b"\x1b[Ha\x1b[1;5Hb");
        assert_eq!(snapshot.lines[0], "a   b");
        assert_eq!(snapshot.row_cells[0], 5);
    }

    #[test]
    fn a_style_run_keeps_a_blank_inside_it() {
        let snapshot = snapshot(2, 6, b"\x1b[7m a\x1b[0m");
        let run = snapshot
            .styles
            .iter()
            .find(|run| run.inverse)
            .expect("the inverse run");
        assert_eq!(run.text, " a");
        assert_eq!(run.start, 0);
        assert_eq!(run.len, 2);
    }

    #[test]
    fn captures_text_and_trims_trailing_spaces() {
        let snapshot = snapshot(3, 10, b"hello\r\nworld");
        assert_eq!(snapshot.lines, ["hello", "world", ""]);
        assert_eq!(snapshot.row_cells, [5, 5, 0]);
        assert!(snapshot.contains("world"));
        assert_eq!(snapshot.text(), "hello\nworld\n\n");
    }

    #[test]
    fn a_row_that_reaches_the_edge_is_reported_only_for_prose() {
        let cut = snapshot(2, 5, b"abcdef");
        assert_eq!(cut.edge_rows, [0]);

        let border = snapshot(2, 5, "\u{250c}\u{2500}\u{2500}\u{2500}\u{2510}".as_bytes());
        assert!(border.edge_rows.is_empty());
    }

    #[test]
    fn records_inverse_and_colour_runs() {
        let snapshot = snapshot(2, 10, b"\x1b[7mselected\x1b[0m");
        let run = snapshot.styles.iter().find(|run| run.inverse).unwrap();
        assert_eq!(run.text, "selected");
        assert_eq!(run.start, 0);
        assert_eq!(run.len, 8);
    }

    #[test]
    fn an_alternate_screen_is_named() {
        let snapshot = snapshot(2, 10, b"\x1b[?1049halb");
        assert!(snapshot.alternate_screen);
        assert!(snapshot.contains("alb"));
    }

    #[test]
    fn a_screen_with_nothing_on_it_is_flagged() {
        let snapshot = snapshot(2, 10, b"\x1b[2J");
        assert!(snapshot
            .warnings
            .iter()
            .any(|warning| matches!(warning, Warning::BlankScreen)));
    }
}
