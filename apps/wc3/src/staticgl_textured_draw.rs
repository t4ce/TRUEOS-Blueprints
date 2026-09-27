// Admission rules for the existing native sampled shader.
fn gl_texture_draw_image(textures: &GlTextures) -> Result<&GlTextureImage, ProviderDispatchError> {
    const API: &str = "glDrawElements";
    let object = textures.object();
    if object.min_filter != 0x2600
        || object.mag_filter != 0x2600
        || object.wrap_s != 0x2901
        || object.wrap_t != 0x2901
    {
        return Err(gl_texture_error(
            API,
            format!(
                "sampled GPU needs nearest/repeat; min=0x{:x} mag=0x{:x} wrap=0x{:x}/0x{:x}",
                object.min_filter, object.mag_filter, object.wrap_s, object.wrap_t
            ),
        ));
    }
    let image = object
        .levels
        .get(&0)
        .ok_or_else(|| gl_texture_error(API, "texture level zero undefined"))?;
    if image.width == 0 || image.height == 0 || !matches!(image.internal, 0x1907 | 0x1908) {
        return Err(gl_texture_error(
            API,
            "sampled GPU needs a nonempty RGB/RGBA image",
        ));
    }
    if !image.rgba.chunks_exact(4).all(|p| p[3] == 255) {
        return Err(gl_texture_error(
            API,
            "sampled GPU currently requires opaque texture pixels",
        ));
    }
    if !matches!(textures.env_mode, 0x2100 | 0x2101 | 0x1e01) {
        return Err(gl_texture_error(
            API,
            "texture environment needs additional shader combine support",
        ));
    }
    Ok(image)
}

// Called only for the admitted opaque RGB/RGBA texture. GL 1.1 tables 3.10/3.11:
// RGB REPLACE and either DECAL preserve primary alpha; RGBA REPLACE does not.
fn gl_texture_primary_color_supported(internal: u32, env: u32, color: [f32; 4]) -> bool {
    if !matches!(internal, 0x1907 | 0x1908) || !color.iter().all(|v| v.is_finite()) {
        return false;
    }
    match env {
        0x2100 => color == [1.0; 4],
        0x1e01 if internal == 0x1908 => true,
        0x1e01 | 0x2101 => color[3] == 1.0,
        _ => false,
    }
}
