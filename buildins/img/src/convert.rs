//! Conversion uses the decoded source format and full-resolution pixels.
extern crate alloc;
use alloc::{format, string::String, vec::Vec};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Format {
    Png,
    Jpeg,
}

pub fn target(source: &str, format: Option<Format>) -> Option<(String, Format)> {
    if source.starts_with("kernel:") || source == "<empty>" {
        return None;
    }
    let output = match format? {
        Format::Png => Format::Jpeg,
        Format::Jpeg => Format::Png,
    };
    let (directory, name) = source
        .rsplit_once('/')
        .map_or(("", source), |(dir, name)| (&source[..dir.len() + 1], name));
    let stem = name
        .rsplit_once('.')
        .filter(|(stem, _)| !stem.is_empty())
        .map_or(name, |(stem, _)| stem);
    let extension = match output {
        Format::Png => "png",
        Format::Jpeg => "jpg",
    };
    let path = format!("{directory}{stem}.{extension}");
    // A mislabeled input must never be overwritten by its conversion.
    (path != source).then_some((path, output))
}

pub fn selected_index(selected: impl Iterator<Item = bool>) -> Option<usize> {
    let mut count = 0;
    let mut chosen = None;
    for (index, selected) in selected.enumerate() {
        count += 1;
        if selected {
            if chosen.is_some() {
                return None;
            }
            chosen = Some(index);
        }
    }
    if count == 1 { Some(0) } else { chosen }
}

pub fn encode(format: Format, width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4));
    if width == 0 || height == 0 || expected != Some(rgba.len()) {
        return Err(String::from("invalid RGBA dimensions"));
    }
    let mut bytes = Vec::new();
    match format {
        Format::Png => {
            let mut filtered = Vec::with_capacity(rgba.len() + height as usize);
            for row in rgba.chunks_exact(width as usize * 4) {
                filtered.push(0); // PNG filter: None.
                filtered.extend_from_slice(row);
            }
            let compressed = miniz_oxide::deflate::compress_to_vec_zlib(&filtered, 6);
            bytes.extend_from_slice(b"\x89PNG\r\n\x1a\n");
            let mut header = Vec::with_capacity(13);
            header.extend_from_slice(&width.to_be_bytes());
            header.extend_from_slice(&height.to_be_bytes());
            header.extend_from_slice(&[8, 6, 0, 0, 0]); // RGBA8, non-interlaced.
            png_chunk(&mut bytes, b"IHDR", &header);
            png_chunk(&mut bytes, b"IDAT", &compressed);
            png_chunk(&mut bytes, b"IEND", &[]);
        }
        Format::Jpeg => {
            let width =
                u16::try_from(width).map_err(|_| String::from("JPEG width exceeds 65535"))?;
            let height =
                u16::try_from(height).map_err(|_| String::from("JPEG height exceeds 65535"))?;
            // Match the viewer's black background when dropping PNG alpha.
            let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
            for pixel in rgba.chunks_exact(4) {
                for channel in &pixel[..3] {
                    rgb.push(((*channel as u16 * pixel[3] as u16 + 127) / 255) as u8);
                }
            }
            jpeg_encoder::Encoder::new(&mut bytes, 90)
                .encode(&rgb, width, height, jpeg_encoder::ColorType::Rgb)
                .map_err(|error| format!("JPEG encode: {error}"))?;
        }
    }
    Ok(bytes)
}

fn png_chunk(output: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    output.extend_from_slice(&(data.len() as u32).to_be_bytes());
    output.extend_from_slice(kind);
    output.extend_from_slice(data);
    let mut crc = crc32fast::Hasher::new();
    crc.update(kind);
    crc.update(data);
    output.extend_from_slice(&crc.finalize().to_be_bytes());
}
