use lz_fear::framed::{CompressionSettings, LZ4FrameReader};
use std::io::Cursor;

fn main() {
    let input = Cursor::new(&[]);
    let mut output = Vec::new();

    CompressionSettings::default()
        .content_checksum(true)
        .independent_blocks(true)
        .compress(input, &mut output)
        .expect("Could not compress input data");

    println!("{:x?}", output);

    let mut lz4_reader = LZ4FrameReader::new(Cursor::new(output))
        .expect("Could not create frame reader")
        .into_read();
}
