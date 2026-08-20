use std::env;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=assets/skybox8k.png");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    let texture = read_png_rgb(include_bytes!("assets/skybox8k.png"));
    let rgb565_path = out_dir.join("skybox_rgb565.bin");
    let meta_path = out_dir.join("skybox_meta.rs");

    let mut packed = Vec::with_capacity(texture.width as usize * texture.height as usize * 2);
    for rgb in texture.pixels.chunks_exact(3) {
        let r = (rgb[0] as u16 >> 3) & 0x1f;
        let g = (rgb[1] as u16 >> 2) & 0x3f;
        let b = (rgb[2] as u16 >> 3) & 0x1f;
        let value = (r << 11) | (g << 5) | b;
        packed.extend_from_slice(&value.to_le_bytes());
    }
    File::create(&rgb565_path)
        .unwrap()
        .write_all(&packed)
        .unwrap();

    let meta = format!(
        "const SKYBOX_SOURCE_WIDTH: usize = {};\nconst SKYBOX_SOURCE_HEIGHT: usize = {};\nconst SKYBOX_WIDTH: usize = {};\nconst SKYBOX_HEIGHT: usize = {};\n",
        texture.width, texture.height, texture.width, texture.height
    );
    File::create(&meta_path)
        .unwrap()
        .write_all(meta.as_bytes())
        .unwrap();
}

struct RgbImage {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

fn read_png_rgb(bytes: &[u8]) -> RgbImage {
    let mut decoder = png::Decoder::new(png::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().unwrap();
    let mut buffer = vec![0u8; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buffer).unwrap();
    let bytes = &buffer[..info.buffer_size()];
    let channels = match info.color_type {
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Indexed => panic!("indexed PNG palette was not expanded"),
    };

    let mut pixels = Vec::with_capacity(info.width as usize * info.height as usize * 3);
    for pixel in bytes.chunks_exact(channels) {
        match channels {
            1 | 2 => pixels.extend_from_slice(&[pixel[0], pixel[0], pixel[0]]),
            3 | 4 => pixels.extend_from_slice(&[pixel[0], pixel[1], pixel[2]]),
            _ => unreachable!(),
        }
    }

    RgbImage {
        width: info.width,
        height: info.height,
        pixels,
    }
}
