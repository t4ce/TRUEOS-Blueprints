#[path = "convert.rs"]
mod convert;
use convert::{Format, encode, selected_index, target};

#[test]
fn selection_requires_one_frame_or_one_selected_frame() {
    for (selection, expected) in [
        (vec![], None),
        (vec![false], Some(0)),
        (vec![true], Some(0)),
        (vec![false, false], None),
        (vec![false, true], Some(1)),
        (vec![true, false], Some(0)),
        (vec![true, true], None),
    ] {
        assert_eq!(selected_index(selection.into_iter()), expected);
    }
}

#[test]
fn paths_replace_extensions_and_use_decoded_format() {
    for (source, format, output) in [
        ("apps/common/a.png", Format::Png, "apps/common/a.jpg"),
        ("apps/common/a.JPEG", Format::Jpeg, "apps/common/a.png"),
        ("apps/common/a.jpg", Format::Jpeg, "apps/common/a.png"),
        (
            "dir.with.dots/a.b.PNG",
            Format::Png,
            "dir.with.dots/a.b.jpg",
        ),
        ("/images/asset", Format::Png, "/images/asset.jpg"),
        ("a.wrong", Format::Jpeg, "a.png"),
    ] {
        assert_eq!(target(source, Some(format)).unwrap().0, output);
    }
    assert_eq!(target("kernel:logo", Some(Format::Png)), None);
    assert_eq!(target("<empty>", None), None);
    assert_eq!(target("a.jpg", Some(Format::Png)), None);
}

#[test]
fn png_roundtrip_preserves_dimensions_and_rgba() {
    let rgba = [255, 0, 40, 255, 20, 30, 50, 0, 0, 200, 1, 128, 1, 2, 3, 255];
    let bytes = encode(Format::Png, 2, 2, &rgba).unwrap();
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .unwrap();
    let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut pixels).unwrap();
    assert_eq!((info.width, info.height), (2, 2));
    assert_eq!(pixels, rgba);
}

#[test]
fn jpeg_roundtrip_composites_alpha_on_black() {
    for (pixel, expected) in [
        ([240, 240, 240, 255], 240u8),
        ([240, 240, 240, 128], 120),
        ([240, 240, 240, 0], 0),
    ] {
        let bytes = encode(Format::Jpeg, 8, 8, &pixel.repeat(64)).unwrap();
        assert!(bytes.starts_with(&[0xff, 0xd8, 0xff]));
        let mut decoder = zune_jpeg::JpegDecoder::new(zune_core::bytestream::ZCursor::new(bytes));
        let pixels = decoder.decode().unwrap();
        let info = decoder.info().unwrap();
        assert_eq!((info.width, info.height), (8, 8));
        assert!(pixels.iter().all(|value| value.abs_diff(expected) <= 2));
    }
}

#[test]
fn invalid_buffers_and_jpeg_dimensions_fail_before_encoding() {
    assert!(encode(Format::Png, 0, 1, &[]).is_err());
    assert!(encode(Format::Jpeg, 2, 2, &[0; 4]).is_err());
    assert!(encode(Format::Jpeg, 65536, 1, &vec![0; 65536 * 4]).is_err());
}
