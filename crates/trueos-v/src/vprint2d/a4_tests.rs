use super::*;

fn pixel(page: &Page, x: u32, y: u32) -> [u8; 3] {
    page.rgb()[(y as usize * WIDTH as usize + x as usize) * 3..][..3]
        .try_into()
        .unwrap()
}

/// Independent PWG reader used to inspect encoded pixel locations and headers.
pub(super) fn decode(document: &[u8]) -> Vec<Vec<u8>> {
    assert_eq!(&document[..4], b"RaS2");
    let mut offset = 4usize;
    let mut pages = Vec::new();
    while offset < document.len() {
        let header = &document[offset..offset + HEADER_BYTES];
        let value = |at: usize| u32::from_be_bytes(header[at..at + 4].try_into().unwrap());
        assert_eq!((value(276), value(280)), (300, 300));
        assert_eq!((value(372), value(376)), (2480, 3508));
        assert_eq!(
            (value(388), value(392), value(400), value(420)),
            (24, WIDTH * 3, 19, 3)
        );
        assert_eq!(&header[1732..1748], b"iso_a4_210x297mm");
        offset += HEADER_BYTES;
        let mut page = Vec::with_capacity(WIDTH as usize * HEIGHT as usize * 3);
        while page.len() < WIDTH as usize * HEIGHT as usize * 3 {
            let repeats = document[offset] as usize + 1;
            offset += 1;
            let mut row = Vec::with_capacity(WIDTH as usize * 3);
            while row.len() < WIDTH as usize * 3 {
                let code = document[offset];
                offset += 1;
                if code < 128 {
                    let value = &document[offset..offset + 3];
                    offset += 3;
                    for _ in 0..=code {
                        row.extend_from_slice(value);
                    }
                } else {
                    assert_ne!(code, 128);
                    let bytes = (257 - code as usize) * 3;
                    row.extend_from_slice(&document[offset..offset + bytes]);
                    offset += bytes;
                }
            }
            assert_eq!(row.len(), WIDTH as usize * 3);
            for _ in 0..repeats {
                page.extend_from_slice(&row);
            }
        }
        assert_eq!(page.len(), WIDTH as usize * HEIGHT as usize * 3);
        pages.push(page);
    }
    pages
}

#[test]
fn physical_a4_and_five_mm_coordinates_do_not_accumulate_rounding_error() {
    assert_eq!((mm_to_px(210.0), mm_to_px(297.0)), (WIDTH, HEIGHT));
    assert_eq!(mm_to_px(5.0), 59);
    assert_eq!(mm_to_px(195.0), 2303);
    assert_eq!(mm_to_px(100.0), 1181);
    let gaps: Vec<_> = (0..39)
        .map(|n| mm_to_px((n + 1) as f32 * 5.0) - mm_to_px(n as f32 * 5.0))
        .collect();
    assert!(gaps.iter().all(|gap| [59, 60].contains(gap)));
    assert_eq!(gaps.iter().sum::<u32>(), mm_to_px(195.0));
}

#[test]
fn image_fit_preserves_aspect_centres_and_alpha_without_viewport_state() {
    let mut page = Page::white().unwrap();
    let rgba = [255, 0, 0, 255, 0, 0, 255, 128];
    page.fit_image_centered(&rgba, 2, 1, 10).unwrap();
    assert_eq!(pixel(&page, 0, 0), [255; 3]);
    let margin = mm_to_px(10.0);
    let w = WIDTH - 2 * margin;
    let h = w / 2;
    let top = (HEIGHT - h) / 2;
    assert_eq!(pixel(&page, margin, top), [255, 0, 0]);
    assert_eq!(pixel(&page, WIDTH - margin - 1, top), [127, 127, 255]);
    assert_eq!(pixel(&page, margin, top - 1), [255; 3]);
    assert_eq!(pixel(&page, margin - 1, top), [255; 3]);
    assert!(page.fit_image_centered(&rgba, 2, 1, 150).is_err());
}

#[test]
fn full_rgb_encoder_roundtrips_lines_pixels_and_repeated_rows() {
    let mut page = Page::white().unwrap();
    page.line(0, 0, WIDTH - 1, 0, [0, 0, 0]);
    for x in 0..WIDTH {
        page.pixel(
            x,
            100,
            [
                (x % 251) as u8,
                ((x * 3) % 251) as u8,
                ((x * 7) % 251) as u8,
            ],
        );
    }
    page.line(70, 40, 90, 140, [12, 56, 90]);
    let mut document = Document::new();
    document.push(&page).unwrap();
    assert_eq!(decode(document.bytes())[0], page.rgb());
}

#[test]
fn multipage_document_has_real_page_counts_and_bounded_atomic_append() {
    let page = Page::white().unwrap();
    let mut doc = Document::new();
    doc.push(&page).unwrap();
    let second = doc.bytes().len();
    doc.push(&page).unwrap();
    assert_eq!(doc.pages(), 2);
    assert_eq!(&doc.bytes()[4 + 452..4 + 456], &2u32.to_be_bytes());
    assert_eq!(
        &doc.bytes()[second + 452..second + 456],
        &2u32.to_be_bytes()
    );
    assert_eq!(decode(doc.bytes()).len(), 2);
    for _ in 2..MAX_PAGES {
        doc.push(&page).unwrap();
    }
    let before = doc.bytes().len();
    assert!(doc.push(&page).is_err());
    assert_eq!(before, doc.bytes().len());
}

fn solid_glyph(_: char) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&[0, 0, 0, 255]);
    Ok(bytes)
}

#[test]
fn text_wraps_paginates_and_reads_unicode_with_cached_native_glyphs() {
    let columns = (WIDTH - 2 * mm_to_px(10.0)) / 25;
    let rows = (HEIGHT - 2 * mm_to_px(10.0)) / 50;
    let text = "é".repeat((columns * rows + 1) as usize);
    let mut calls = 0;
    let doc = render_text_with(&text, |ch| {
        assert_eq!(ch, 'é');
        calls += 1;
        solid_glyph(ch)
    })
    .unwrap();
    assert_eq!(doc.pages(), 2);
    assert_eq!(calls, 1);
    let pages = decode(doc.bytes());
    let x = mm_to_px(10.0) as usize;
    let at = (x * WIDTH as usize + x) * 3;
    assert_eq!(&pages[1][at..at + 3], &[0, 0, 0]);
}

#[test]
fn text_crlf_tabs_formfeeds_blanklines_and_trailing_newline_have_stable_pages() {
    let rows = (HEIGHT - 2 * mm_to_px(10.0)) / 50;
    let a = render_text_with("A\r\n\tB", solid_glyph).unwrap();
    let b = render_text_with("A\n    B", solid_glyph).unwrap();
    assert_eq!(a.bytes(), b.bytes());
    assert_eq!(
        render_text_with("A\u{000c}B", solid_glyph).unwrap().pages(),
        2
    );
    let boundary = format!("{}\u{000c}B", "A\n".repeat(rows as usize));
    assert_eq!(render_text_with(&boundary, solid_glyph).unwrap().pages(), 2);
    assert_eq!(render_text_with("A\n", solid_glyph).unwrap().pages(), 1);
    assert_eq!(
        render_text_with(&"\n".repeat(rows as usize + 1), solid_glyph)
            .unwrap()
            .pages(),
        2
    );
    assert_eq!(render_text_with("", solid_glyph).unwrap().pages(), 1);
}

#[test]
fn invalid_image_dimensions_or_glyph_payload_fail_before_submission() {
    let mut page = Page::white().unwrap();
    assert!(
        page.blit_rgba(&[0; 4], u32::MAX, u32::MAX, 0, 0, 1, 1)
            .is_err()
    );
    assert!(page.blit_rgba(&[0; 3], 1, 1, 0, 0, 1, 1).is_err());
    assert!(render_text_with("A", |_| Ok(vec![0; 7])).is_err());
}
