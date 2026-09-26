// Context-local OpenGL 1.1 texture objects. Guest pixels are owned after upload;
// GPU storage is materialized at draw, never by glGenTextures.
const GL_TEXTURE_2D: u32 = 0x0de1;
const GL_TEXTURE_COORD_ARRAY: u32 = 0x8078;
const GL_MAX_TEXTURE_EDGE: u32 = 4096;
const GL_TEXTURE_BUDGET: usize = 256 * 1024 * 1024;
const GL_TEXTURE_NAME_LIMIT: usize = 65536;

#[derive(Clone, Copy, Debug)]
struct GlUnpack {
    alignment: u32,
    row_length: u32,
    skip_rows: u32,
    skip_pixels: u32,
    swap_bytes: bool,
    lsb_first: bool,
}
impl Default for GlUnpack {
    fn default() -> Self {
        Self {
            alignment: 4,
            row_length: 0,
            skip_rows: 0,
            skip_pixels: 0,
            swap_bytes: false,
            lsb_first: false,
        }
    }
}
#[derive(Debug)]
struct GlTextureImage {
    width: u32,
    height: u32,
    internal: u32,
    rgba: Vec<u8>,
}
#[derive(Debug)]
struct GlTextureObject {
    levels: HashMap<u32, GlTextureImage>,
    min_filter: u32,
    mag_filter: u32,
    wrap_s: u32,
    wrap_t: u32,
}
impl Default for GlTextureObject {
    fn default() -> Self {
        Self {
            levels: HashMap::new(),
            min_filter: 0x2702,
            mag_filter: 0x2601,
            wrap_s: 0x2901,
            wrap_t: 0x2901,
        }
    }
}
#[derive(Debug)]
struct GlTextures {
    reserved: HashSet<u32>,
    objects: HashMap<u32, GlTextureObject>,
    next: u32,
    binding: u32,
    default_object: GlTextureObject,
    unpack: GlUnpack,
    enabled: bool,
    coord_array_enabled: bool,
    coord_pointer: Option<GlArrayPointer>,
    env_mode: u32,
}
impl Default for GlTextures {
    fn default() -> Self {
        Self {
            reserved: HashSet::new(),
            objects: HashMap::new(),
            next: 1,
            binding: 0,
            default_object: GlTextureObject::default(),
            unpack: GlUnpack::default(),
            enabled: false,
            coord_array_enabled: false,
            coord_pointer: None,
            env_mode: 0x2100,
        }
    }
}
impl GlTextures {
    fn object(&self) -> &GlTextureObject {
        if self.binding == 0 {
            &self.default_object
        } else {
            &self.objects[&self.binding]
        }
    }
    fn object_mut(&mut self) -> &mut GlTextureObject {
        if self.binding == 0 {
            &mut self.default_object
        } else {
            self.objects.get_mut(&self.binding).unwrap()
        }
    }
    fn bytes(&self) -> usize {
        self.objects
            .values()
            .chain(std::iter::once(&self.default_object))
            .flat_map(|o| o.levels.values())
            .map(|i| i.rgba.len())
            .sum()
    }
    fn plan_names(&self, count: usize) -> Result<(Vec<u32>, u32), &'static str> {
        if count > GL_TEXTURE_NAME_LIMIT || self.reserved.len() + count > GL_TEXTURE_NAME_LIMIT {
            return Err("texture namespace budget exceeded");
        }
        let mut next = self.next.max(1);
        let mut names = Vec::with_capacity(count);
        while names.len() < count {
            if !self.reserved.contains(&next) {
                names.push(next);
            }
            next = next.wrapping_add(1).max(1);
        }
        Ok((names, next))
    }
    fn generate(
        &mut self,
        count: usize,
        output: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<(), &'static str> {
        let (names, next) = self.plan_names(count)?;
        if count != 0 {
            if output == 0 {
                return Err("null texture-name output");
            }
            output
                .checked_add((count * 4 - 1) as u32)
                .ok_or("texture-name output overflow")?;
            let bytes: Vec<u8> = names.iter().flat_map(|n| n.to_le_bytes()).collect();
            memory.write(output, &bytes)?;
        }
        self.reserved.extend(names);
        self.next = next;
        Ok(())
    }
    fn bind(&mut self, name: u32) -> Result<(), &'static str> {
        if name != 0 {
            if !self.reserved.contains(&name) && self.reserved.len() >= GL_TEXTURE_NAME_LIMIT {
                return Err("texture namespace budget exceeded");
            }
            self.reserved.insert(name);
            self.objects.entry(name).or_default();
        }
        self.binding = name;
        Ok(())
    }
    fn delete(&mut self, names: &[u32]) {
        for &name in names {
            if name != 0 {
                self.reserved.remove(&name);
                self.objects.remove(&name);
                if self.binding == name {
                    self.binding = 0;
                }
            }
        }
    }
    fn set_image(&mut self, level: u32, image: GlTextureImage) -> Result<(), &'static str> {
        let old = self.object().levels.get(&level).map_or(0, |i| i.rgba.len());
        if self.bytes() - old + image.rgba.len() > GL_TEXTURE_BUDGET {
            return Err("texture pixel budget exceeded");
        }
        self.object_mut().levels.insert(level, image);
        Ok(())
    }
}
impl GlTextures {
    fn sub_image(
        &mut self,
        level: u32,
        rect: [u32; 4],
        format: u32,
        kind: u32,
        pixels: u32,
        memory: &impl GuestMemory,
    ) -> Result<(), &'static str> {
        let [x, y, width, height] = rect;
        let image = self
            .object()
            .levels
            .get(&level)
            .ok_or("undefined image level")?;
        if x.checked_add(width).is_none_or(|end| end > image.width)
            || y.checked_add(height).is_none_or(|end| end > image.height)
            || (width != 0 && height != 0 && pixels == 0)
        {
            return Err("invalid subimage bounds/pointer");
        }
        // Decode the entire source first: a failed later row must not leave a
        // partially changed texture visible to the next guest draw.
        let rgba = gl_texture_pixels(
            memory,
            pixels,
            width,
            height,
            format,
            kind,
            self.unpack,
            image.internal,
        )?;
        let image = self.object_mut().levels.get_mut(&level).unwrap();
        for row in 0..height as usize {
            let start = ((y as usize + row) * image.width as usize + x as usize) * 4;
            let bytes = width as usize * 4;
            image.rgba[start..start + bytes].copy_from_slice(&rgba[row * bytes..(row + 1) * bytes]);
        }
        Ok(())
    }
}

fn gl_texture_error(api: &'static str, detail: impl Into<String>) -> ProviderDispatchError {
    ProviderDispatchError::Frontier {
        api,
        detail: detail.into(),
    }
}
fn gl_texture_target(target: u32, api: &'static str) -> Result<(), ProviderDispatchError> {
    if target != GL_TEXTURE_2D {
        return Err(gl_texture_error(
            api,
            format!("target=0x{target:x}; only TEXTURE_2D supported"),
        ));
    }
    Ok(())
}
fn gl_texture_internal(internal: u32) -> Result<u32, &'static str> {
    // Canonical storage keeps 8-bit components. Sized formats requiring another
    // precision stay explicit until their conversion is implemented.
    match internal {
        1 | 0x1909 | 0x8040 => Ok(0x1909),
        2 | 0x190a | 0x8045 => Ok(0x190a),
        3 | 0x1907 | 0x8051 => Ok(0x1907),
        4 | 0x1908 | 0x8058 => Ok(0x1908),
        0x1906 | 0x803c => Ok(0x1906),
        0x8049 | 0x804b => Ok(0x8049),
        _ => Err("unsupported texture internal precision/format"),
    }
}
fn gl_texture_convert_internal(pixel: [u8; 4], internal: u32) -> [u8; 4] {
    let [r, g, b, a] = pixel;
    match internal {
        0x1906 => [255, 255, 255, a],
        0x1909 => [r, r, r, 255],
        0x190a => [r, r, r, a],
        0x8049 => [r, r, r, r],
        0x1907 => [r, g, b, 255],
        _ => pixel,
    }
}
fn gl_texture_pixels(
    memory: &impl GuestMemory,
    address: u32,
    width: u32,
    height: u32,
    format: u32,
    kind: u32,
    unpack: GlUnpack,
    internal: u32,
) -> Result<Vec<u8>, &'static str> {
    let channels: usize = match format {
        0x1903..=0x1906 | 0x1909 => 1,
        0x190a => 2,
        0x1907 | 0x80e0 => 3,
        0x1908 | 0x80e1 => 4,
        _ => return Err("unsupported source pixel format"),
    };
    if kind != GL_UNSIGNED_BYTE {
        return Err("texture upload supports unsigned-byte components only");
    }
    if width > GL_MAX_TEXTURE_EDGE || height > GL_MAX_TEXTURE_EDGE {
        return Err("texture dimensions exceed limit");
    }
    let len = width as usize * height as usize * 4;
    let mut rgba = vec![0; len];
    if width == 0 || height == 0 {
        return Ok(rgba);
    }
    // A null TexImage pointer allocates undefined storage; deterministic zero is
    // permitted. SubImage checks null before reaching this helper.
    if address == 0 {
        return Ok(rgba);
    }
    let row_pixels = if unpack.row_length == 0 {
        width
    } else {
        unpack.row_length
    } as u64;
    let row_bytes = row_pixels * channels as u64;
    let alignment = unpack.alignment as u64;
    let stride = (row_bytes + alignment - 1) & !(alignment - 1);
    let start = (unpack.skip_rows as u64)
        .checked_mul(stride)
        .and_then(|v| v.checked_add(unpack.skip_pixels as u64 * channels as u64))
        .ok_or("texture source address overflow")?;
    let end = (address as u64)
        .checked_add(start)
        .and_then(|v| {
            (height as u64 - 1)
                .checked_mul(stride)
                .and_then(|rows| v.checked_add(rows))
        })
        .and_then(|v| v.checked_add(width as u64 * channels as u64))
        .ok_or("texture source address overflow")?;
    if end > u32::MAX as u64 + 1 {
        return Err("texture source address overflow");
    }
    let mut row = vec![0; width as usize * channels];
    for y in 0..height as usize {
        let source = (address as u64 + start + y as u64 * stride) as u32;
        memory.read(source, &mut row)?;
        for x in 0..width as usize {
            let p = &row[x * channels..(x + 1) * channels];
            let pixel = match format {
                0x1903 => [p[0], 0, 0, 255],
                0x1904 => [0, p[0], 0, 255],
                0x1905 => [0, 0, p[0], 255],
                0x1906 => [0, 0, 0, p[0]],
                0x1909 => [p[0], p[0], p[0], 255],
                0x190a => [p[0], p[0], p[0], p[1]],
                0x1907 => [p[0], p[1], p[2], 255],
                0x80e0 => [p[2], p[1], p[0], 255],
                0x80e1 => [p[2], p[1], p[0], p[3]],
                _ => [p[0], p[1], p[2], p[3]],
            };
            rgba[(y * width as usize + x) * 4..(y * width as usize + x + 1) * 4]
                .copy_from_slice(&gl_texture_convert_internal(pixel, internal));
        }
    }
    Ok(rgba)
}
impl XpProcess {
    fn gl_gen_textures_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, count, output] = arguments::<3>(memory, esp)?;
        self.gl_context_mut(tid, "glGenTextures")?
            .textures
            .generate(count as usize, output, memory)
            .map_err(|e| gl_texture_error("glGenTextures", e))?;
        logl::log(
            level::IMPORTANT,
            format_args!(
                "WC3 GL TEXTURE NAMES tid={tid} count={count} output=0x{output:08x} storage=unallocated"
            ),
        );
        Ok(0)
    }
    fn gl_bind_texture_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, target, name] = arguments::<3>(memory, esp)?;
        gl_texture_target(target, "glBindTexture")?;
        self.gl_context_mut(tid, "glBindTexture")?
            .textures
            .bind(name)
            .map_err(|e| gl_texture_error("glBindTexture", e))?;
        Ok(0)
    }
    fn gl_delete_textures_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, count, address] = arguments::<3>(memory, esp)?;
        if count as usize > GL_TEXTURE_NAME_LIMIT {
            return Err(gl_texture_error("glDeleteTextures", "count exceeds bound"));
        }
        let mut bytes = vec![0; count as usize * 4];
        if count != 0 {
            if address == 0 {
                return Err(gl_texture_error("glDeleteTextures", "null names"));
            }
            address
                .checked_add(count * 4 - 1)
                .ok_or("texture names overflow")?;
            memory.read(address, &mut bytes)?;
        }
        let names: Vec<u32> = bytes
            .chunks_exact(4)
            .map(|p| u32::from_le_bytes(p.try_into().unwrap()))
            .collect();
        self.gl_context_mut(tid, "glDeleteTextures")?
            .textures
            .delete(&names);
        Ok(0)
    }
    fn gl_pixel_storei_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, pname, value] = arguments::<3>(memory, esp)?;
        let unpack = &mut self.gl_context_mut(tid, "glPixelStorei")?.textures.unpack;
        match pname {
            0x0cf5 if matches!(value, 1 | 2 | 4 | 8) => unpack.alignment = value,
            0x0cf2 if value <= i32::MAX as u32 => unpack.row_length = value,
            0x0cf3 if value <= i32::MAX as u32 => unpack.skip_rows = value,
            0x0cf4 if value <= i32::MAX as u32 => unpack.skip_pixels = value,
            0x0cf0 => unpack.swap_bytes = value != 0,
            0x0cf1 => unpack.lsb_first = value != 0,
            _ => {
                return Err(gl_texture_error(
                    "glPixelStorei",
                    format!("unsupported pname/value 0x{pname:x}/{value}"),
                ));
            }
        }
        Ok(0)
    }
    fn gl_tex_parameteri_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, target, pname, value] = arguments::<4>(memory, esp)?;
        gl_texture_target(target, "glTexParameteri")?;
        let object = self
            .gl_context_mut(tid, "glTexParameteri")?
            .textures
            .object_mut();
        match pname {
            0x2800 if matches!(value, 0x2600 | 0x2601) => object.mag_filter = value,
            0x2801 if matches!(value, 0x2600 | 0x2601 | 0x2700..=0x2703) => {
                object.min_filter = value
            }
            0x2802 if matches!(value, 0x2900 | 0x2901 | 0x812f) => object.wrap_s = value,
            0x2803 if matches!(value, 0x2900 | 0x2901 | 0x812f) => object.wrap_t = value,
            _ => {
                return Err(gl_texture_error(
                    "glTexParameteri",
                    format!("unsupported pname/value 0x{pname:x}/0x{value:x}"),
                ));
            }
        }
        Ok(0)
    }
    fn gl_tex_envi_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, target, pname, value] = arguments::<4>(memory, esp)?;
        if target != 0x2300
            || pname != 0x2200
            || !matches!(value, 0x2100 | 0x2101 | 0x1e01 | 0x0be2)
        {
            return Err(gl_texture_error(
                "glTexEnvi",
                "unsupported texture environment",
            ));
        }
        self.gl_context_mut(tid, "glTexEnvi")?.textures.env_mode = value;
        Ok(0)
    }
    fn gl_tex_image_2d_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [
            _,
            target,
            level,
            internal,
            width,
            height,
            border,
            format,
            kind,
            pixels,
        ] = arguments::<10>(memory, esp)?;
        gl_texture_target(target, "glTexImage2D")?;
        if level > 12
            || border != 0
            || width > GL_MAX_TEXTURE_EDGE >> level
            || height > GL_MAX_TEXTURE_EDGE >> level
            || (width != 0 && !width.is_power_of_two())
            || (height != 0 && !height.is_power_of_two())
        {
            return Err(gl_texture_error(
                "glTexImage2D",
                format!("unsupported level={level} dimensions={width}x{height} border={border}"),
            ));
        }
        let internal =
            gl_texture_internal(internal).map_err(|e| gl_texture_error("glTexImage2D", e))?;
        let context = self.gl_context_mut(tid, "glTexImage2D")?;
        let rgba = gl_texture_pixels(
            memory,
            pixels,
            width,
            height,
            format,
            kind,
            context.textures.unpack,
            internal,
        )
        .map_err(|e| gl_texture_error("glTexImage2D", e))?;
        context
            .textures
            .set_image(
                level,
                GlTextureImage {
                    width,
                    height,
                    internal,
                    rgba,
                },
            )
            .map_err(|e| gl_texture_error("glTexImage2D", e))?;
        logl::log(
            level::IMPORTANT,
            format_args!(
                "WC3 GL TEXTURE IMAGE tid={tid} name={} level={level} size={width}x{height} source=0x{pixels:08x} storage=owned-rgba8",
                context.textures.binding
            ),
        );
        Ok(0)
    }
    fn gl_tex_sub_image_2d_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, target, level, x, y, width, height, format, kind, pixels] =
            arguments::<10>(memory, esp)?;
        gl_texture_target(target, "glTexSubImage2D")?;
        self.gl_context_mut(tid, "glTexSubImage2D")?
            .textures
            .sub_image(level, [x, y, width, height], format, kind, pixels, memory)
            .map_err(|e| gl_texture_error("glTexSubImage2D", e))?;
        Ok(0)
    }

    fn gl_tex_coord_pointer_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, size, kind, stride, address] = arguments::<5>(memory, esp)?;
        if !(1..=4).contains(&size) || kind != GL_FLOAT || stride > 4096 {
            return Err(gl_texture_error(
                "glTexCoordPointer",
                "unsupported size/type/stride",
            ));
        }
        self.gl_context_mut(tid, "glTexCoordPointer")?
            .textures
            .coord_pointer = Some(GlArrayPointer {
            size,
            kind,
            stride,
            address,
        });
        Ok(0)
    }
    fn gl_enable_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, cap] = arguments::<2>(memory, esp)?;
        let context = self.gl_context_mut(tid, "glEnable")?;
        match cap {
            GL_TEXTURE_2D => context.textures.enabled = true,
            GL_LIGHT0 => context.light0_enabled = true,
            _ => {
                return Err(gl_texture_error(
                    "glEnable",
                    format!("unmodeled cap=0x{cap:x}"),
                ));
            }
        }
        Ok(0)
    }
    fn gl_get_integerv_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, pname, output] = arguments::<3>(memory, esp)?;
        let c = self.gl_context_mut(tid, "glGetIntegerv")?;
        let values: Vec<u32> = match pname {
            0x0d33 => vec![GL_MAX_TEXTURE_EDGE],
            0x8069 => vec![c.textures.binding],
            0x0cf5 => vec![c.textures.unpack.alignment],
            0x0cf2 => vec![c.textures.unpack.row_length],
            0x0cf3 => vec![c.textures.unpack.skip_rows],
            0x0cf4 => vec![c.textures.unpack.skip_pixels],
            0x0cf0 => vec![c.textures.unpack.swap_bytes as u32],
            0x0cf1 => vec![c.textures.unpack.lsb_first as u32],
            0x0ba0 => vec![c.matrix_mode],
            0x0ba2 => c.viewport.iter().map(|v| *v as u32).collect(),
            _ => {
                return Err(gl_texture_error(
                    "glGetIntegerv",
                    format!("unmodeled pname=0x{pname:x}"),
                ));
            }
        };
        if output == 0 {
            return Err(gl_texture_error("glGetIntegerv", "null output"));
        }
        let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        output
            .checked_add(bytes.len() as u32 - 1)
            .ok_or("GL query output overflow")?;
        memory.write(output, &bytes)?;
        Ok(0)
    }
}

include!("staticgl_texture_tests.rs");
