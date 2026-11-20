use glicol_parser::{HighlightKind, HighlightSpan};
use ratatui::{buffer::CellWidth as _, style::Color};
use std::borrow::Cow;

pub struct DisplaySpan {
    pub start: usize,
    pub end: usize,
    pub color: Color,
}

pub struct Highlight {
    pub lines: Vec<Vec<DisplaySpan>>,
}

impl Highlight {
    pub fn new(code: &str, spans: &[HighlightSpan], tab_length: u8) -> Self {
        let mut spans = spans.iter().peekable();
        let mut line_start = 0;
        let lines = code
            .split('\n')
            .map(|line| {
                let line_end = line_start + line.len();
                let mut offset = line_start;
                let mut columns = Columns::default();
                let mut colors = Vec::new();

                while let Some(span) = spans.peek() {
                    if span.start >= line_end {
                        break;
                    }
                    if span.end <= line_start {
                        spans.next();
                        continue;
                    }
                    let start = span.start.max(line_start);
                    let end = span.end.min(line_end);
                    columns.advance(&code[offset..start], tab_length);
                    let display_start = columns.visible;
                    columns.advance(&code[start..end], tab_length);
                    if display_start < columns.visible {
                        colors.push(DisplaySpan {
                            start: display_start,
                            end: columns.visible,
                            color: match span.kind {
                                HighlightKind::Binding => Color::LightCyan,
                                HighlightKind::Node => Color::LightBlue,
                                HighlightKind::Reference => Color::Cyan,
                                HighlightKind::Number => Color::LightYellow,
                                HighlightKind::Sample => Color::LightMagenta,
                                HighlightKind::Script => Color::LightGreen,
                                HighlightKind::Comment => Color::DarkGray,
                                HighlightKind::Operator => Color::Gray,
                            },
                        });
                    }
                    offset = end;
                    if span.end > line_end {
                        break;
                    }
                    spans.next();
                }
                line_start = line_end + 1;
                colors
            })
            .collect();
        Self { lines }
    }
}

#[derive(Default)]
struct Columns {
    tab_column: usize,
    visible: usize,
}

impl Columns {
    fn advance(&mut self, text: &str, tab_length: u8) {
        for fragment in text.split_inclusive('\t') {
            let content = fragment.strip_suffix('\t').unwrap_or(fragment);
            let printable = if content.chars().any(char::is_control) {
                Cow::Owned(
                    content
                        .chars()
                        .filter(|ch| !ch.is_control())
                        .collect::<String>(),
                )
            } else {
                Cow::Borrowed(content)
            };
            // TextArea calculates tab stops by character and renders terminal graphemes.
            if printable.is_ascii() {
                self.tab_column += printable.len();
                self.visible += printable.len();
            } else {
                self.tab_column += printable
                    .chars()
                    .map(|ch| usize::from(ch.encode_utf8(&mut [0; 4]).cell_width()))
                    .sum::<usize>();
                self.visible += usize::from(printable.as_ref().cell_width());
            }
            if fragment.ends_with('\t') && tab_length > 0 {
                let tab_length = usize::from(tab_length);
                let spaces = tab_length - self.tab_column % tab_length;
                self.tab_column += spaces;
                self.visible += spaces;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glicol_parser::get_ast_with_spans;

    #[test]
    fn colors_come_from_the_same_parse_as_the_components() {
        let code = "// 東京\n~gate: seq 60 _60 ~a;\nout: get ~gate >> sp \\808ch >> meta `\"// script\"`;\n";
        let (ast, spans) = get_ast_with_spans(code).unwrap();
        assert_eq!(ast.nodes["~gate"].len(), 1);
        assert_eq!(ast.nodes["out"].len(), 3);
        assert!(spans.windows(2).all(|pair| pair[0].end <= pair[1].start));
        let tokens: Vec<_> = spans
            .iter()
            .map(|span| (span.kind, code[span.start..span.end].trim_end()))
            .collect();
        assert!(tokens.contains(&(HighlightKind::Comment, "// 東京")));
        assert!(tokens.contains(&(HighlightKind::Binding, "~gate")));
        assert!(tokens.contains(&(HighlightKind::Node, "seq")));
        assert!(tokens.contains(&(HighlightKind::Reference, "~a")));
        assert!(tokens.contains(&(HighlightKind::Sample, r"\808ch")));
        assert!(tokens.contains(&(HighlightKind::Number, "60")));
        assert!(tokens.contains(&(HighlightKind::Operator, ">>")));
        assert!(tokens.contains(&(HighlightKind::Script, "`\"// script\"`")));
    }

    #[test]
    fn display_spans_follow_tab_stops_and_unicode_columns() {
        let code = "\t// α🎛️\nout: sin\t440 >> mul 0.1\n";
        let (_, spans) = get_ast_with_spans(code).unwrap();
        let highlight = Highlight::new(code, &spans, 4);
        let comment = &highlight.lines[0][0];
        assert_eq!((comment.start, comment.end), (4, 10));
        let frequency = highlight.lines[1]
            .iter()
            .find(|span| span.color == Color::LightYellow)
            .unwrap();
        assert_eq!((frequency.start, frequency.end), (12, 15));
        assert!(highlight.lines[2].is_empty());
    }

    #[test]
    fn multiline_scripts_clip_to_each_editor_row() {
        let code = "out: meta `first\n\n\t文` >> mul 1;\n";
        let (_, spans) = get_ast_with_spans(code).unwrap();
        let highlight = Highlight::new(code, &spans, 4);
        let script_color = Color::LightGreen;
        let first = highlight.lines[0]
            .iter()
            .find(|span| span.color == script_color)
            .unwrap();
        assert_eq!((first.start, first.end), (10, 16));
        assert!(highlight.lines[1].is_empty());
        let last = &highlight.lines[2][0];
        assert_eq!((last.start, last.end, last.color), (0, 7, script_color));
        let pipeline = &highlight.lines[2][1];
        assert_eq!(
            (pipeline.start, pipeline.end, pipeline.color),
            (8, 10, Color::Gray)
        );
    }
}
