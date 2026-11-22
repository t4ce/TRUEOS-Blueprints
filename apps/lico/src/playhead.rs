use crate::highlight::DisplaySequenceSlot;
use glicol::SequencePlayhead;
use ratatui::layout::Rect;
use ratatui_textarea::TextArea;
use std::fmt::Write;

// The Shell3 cell outline report accepts at most twelve visible rectangles.
const MAX_OUTLINES: usize = 12;

pub fn visible_outlines(
    outlines: &mut Vec<Rect>,
    patch: &TextArea<'_>,
    area: Rect,
    sequences: &[Vec<DisplaySequenceSlot>],
    positions: &[Option<SequencePlayhead>],
) {
    let area = patch.block().map_or(area, |block| block.inner(area));
    let (top, left) = patch.scroll_offset();
    let (top, left) = (usize::from(top), usize::from(left));
    let gutter = usize::from(patch.line_number_width());
    let width = usize::from(area.width);
    for (slots, position) in sequences.iter().zip(positions) {
        let Some(position) = position else {
            continue;
        };
        let next = slots.partition_point(|slot| {
            (f64::from(slot.at) * position.bar_length) as usize <= position.bar_frame
        });
        let Some(slot) = next.checked_sub(1).and_then(|index| slots.get(index)) else {
            continue;
        };
        let Some(row) = slot.row.checked_sub(top) else {
            continue;
        };
        let start = (gutter + slot.start).saturating_sub(left).min(width);
        let end = (gutter + slot.end).saturating_sub(left).min(width);
        if row < usize::from(area.height) && start < end {
            outlines.push(Rect::new(
                area.x + start as u16,
                area.y + row as u16,
                (end - start) as u16,
                1,
            ));
            if outlines.len() == MAX_OUTLINES {
                break;
            }
        }
    }
}

pub fn publish(outlines: &[Rect]) -> bool {
    let report = report(outlines);
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    {
        // Keep the report in one write so SSH can consume it as local-only metadata.
        v::vshell::write(report.as_bytes()) == report.len()
    }
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    {
        let _ = report;
        false
    }
}

fn report(outlines: &[Rect]) -> String {
    let mut report = format!("\x1b]777;cell-outlines;1;{}", outlines.len());
    for rect in outlines {
        write!(
            report,
            ";{};{};{};{}",
            rect.x, rect.y, rect.width, rect.height
        )
        .expect("write to string");
    }
    report.push('\x07');
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::highlight::Highlight;
    use ratatui::{Terminal, backend::TestBackend, widgets::Block};

    #[test]
    fn outlines_follow_notes_and_rests_then_wrap() {
        let code = "out: seq 60 _60 63;";
        let (_, source) = glicol_parser::get_ast_with_source(code).unwrap();
        let mut patch = TextArea::from([code]);
        patch.set_block(Block::bordered());
        patch.set_line_number_style(Default::default());
        let slots = Highlight::sequence_slots(code, &source.sequences, patch.tab_length());
        let area = Rect::new(2, 1, 24, 5);
        let mut terminal = Terminal::new(TestBackend::new(30, 7)).unwrap();
        terminal
            .draw(|frame| frame.render_widget(&patch, area))
            .unwrap();

        let mut rects = Vec::new();
        for (bar_frame, x, width) in [
            (0, 15, 2),
            (32_001, 18, 1),
            (48_000, 19, 2),
            (70_000, 22, 2),
            (0, 15, 2),
        ] {
            rects.clear();
            visible_outlines(
                &mut rects,
                &patch,
                area,
                &slots,
                &[Some(SequencePlayhead {
                    bar_frame,
                    bar_length: 96_000.0,
                    frames_since_trigger: None,
                })],
            );
            assert_eq!(rects, [Rect::new(x, 2, width, 1)]);
        }
        assert_eq!(report(&rects), "\x1b]777;cell-outlines;1;1;15;2;2;1\x07");
        assert_eq!(report(&[]), "\x1b]777;cell-outlines;1;0\x07");
    }
}
