use std::collections::HashMap;

use crate::{Error, FontDesc, FontKey, GlyphKey, Metrics, Rasterize, RasterizedGlyph, Size};

struct TrueosFace;

pub struct TrueosRasterizer {
    faces: HashMap<FontKey, TrueosFace>,
}

impl Rasterize for TrueosRasterizer {
    fn new() -> Result<Self, Error> {
        Ok(Self { faces: HashMap::new() })
    }

    fn metrics(&self, key: FontKey, _size: Size) -> Result<Metrics, Error> {
        if self.faces.contains_key(&key) {
            Err(Error::PlatformError("TRUEOS font metrics are not wired yet".into()))
        } else {
            Err(Error::UnknownFontKey)
        }
    }

    fn load_font(&mut self, _desc: &FontDesc, _size: Size) -> Result<FontKey, Error> {
        Err(Error::PlatformError("TRUEOS font loading is not wired yet".into()))
    }

    fn get_glyph(&mut self, glyph: GlyphKey) -> Result<RasterizedGlyph, Error> {
        Err(Error::MissingGlyph(RasterizedGlyph {
            character: glyph.character,
            ..RasterizedGlyph::default()
        }))
    }

    fn kerning(&mut self, _left: GlyphKey, _right: GlyphKey) -> (f32, f32) {
        (0.0, 0.0)
    }
}
