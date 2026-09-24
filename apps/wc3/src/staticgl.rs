fn gl_array_component(
    memory: &impl GuestMemory,
    pointer: GlArrayPointer,
    index: u32,
    component: u32,
) -> Result<f32, ProviderDispatchError> {
    let item_bytes = if pointer.kind == GL_FLOAT { 4 } else { 1 };
    let stride = if pointer.stride == 0 {
        pointer.size * item_bytes
    } else {
        pointer.stride
    };
    let offset = index
        .checked_mul(stride)
        .and_then(|offset| {
            component
                .checked_mul(item_bytes)
                .and_then(|component| offset.checked_add(component))
        })
        .ok_or("GL array offset overflow")?;
    let address = pointer
        .address
        .checked_add(offset)
        .ok_or("GL array address overflow")?;
    if pointer.kind == GL_FLOAT {
        let mut bytes = [0u8; 4];
        memory.read(address, &mut bytes)?;
        Ok(f32::from_le_bytes(bytes))
    } else {
        let mut byte = [0u8; 1];
        memory.read(address, &mut byte)?;
        Ok(f32::from(byte[0]) / 255.0)
    }
}

fn gl_transform(matrix: &[f32; 16], point: [f32; 4]) -> [f32; 4] {
    core::array::from_fn(|row| {
        (0..4)
            .map(|column| matrix[column * 4 + row] * point[column])
            .sum()
    })
}

fn gl_rgba8(color: [f32; 4]) -> u32 {
    let bytes = color.map(|component| (component.clamp(0.0, 1.0) * 255.0).round() as u8);
    u32::from_le_bytes(bytes)
}

/* OPENGL32.dll
wglMakeCurrent      iat_rva=0x007061bc
glDisable           iat_rva=0x007061c0
glEnable            iat_rva=0x007061c4
glLightfv           iat_rva=0x007061c8
glFogfv             iat_rva=0x007061cc
glFogf              iat_rva=0x007061d0
glFogi              iat_rva=0x007061d4
glDrawBuffer        iat_rva=0x007061d8
glDepthFunc         iat_rva=0x007061dc
glAlphaFunc         iat_rva=0x007061e0
glBlendFunc         iat_rva=0x007061e4
glEnableClientState  iat_rva=0x007061e8
glTexEnvi               iat_rva=0x007061ec
glBindTexture           iat_rva=0x007061f0
glDisableClientState    iat_rva=0x007061f4
glDepthMask             iat_rva=0x007061f8
glColorMaterial         iat_rva=0x007061fc
glTexGeni               iat_rva=0x00706200
glLightModelfv          iat_rva=0x00706204
glMaterialfv            iat_rva=0x00706208
glPolygonOffset         iat_rva=0x0070620c
glGetIntegerv           iat_rva=0x00706210
wglGetProcAddress  iat_rva=0x00706214
glGetString  iat_rva=0x00706218
wglCreateContext  iat_rva=0x0070621c
wglDeleteContext  iat_rva=0x00706220
glDeleteTextures  iat_rva=0x00706224
glTexSubImage2D  iat_rva=0x00706228
glTexImage2D  iat_rva=0x0070622c
glPixelStorei  iat_rva=0x00706230
glTexParameteri  iat_rva=0x00706234
glGenTextures  iat_rva=0x00706238
glNormal3fv  iat_rva=0x0070623c
glNormalPointer  iat_rva=0x00706240
glVertexPointer  iat_rva=0x00706244
glColorPointer  iat_rva=0x00706248
glTexCoordPointer  iat_rva=0x0070624c
glFinish                iat_rva=0x00706250
glDrawElements         iat_rva=0x00706254
glLoadMatrixf          iat_rva=0x00706258
glMatrixMode           iat_rva=0x0070625c
glScissor              iat_rva=0x00706260
glDepthRange           iat_rva=0x00706264
glViewport             iat_rva=0x00706268
glClear                iat_rva=0x0070626c
glClearColor           iat_rva=0x00706270
glReadPixels           iat_rva=0x00706274
glReadBuffer           iat_rva=0x00706278
wglSwapLayerBuffers    iat_rva=0x0070627c
glLightf               iat_rva=0x00706280
*/

macro_rules! static_gl_stubs {
    ($($method:ident => $api:literal),+ $(,)?) => {
        $(
            fn $method(
                &self,
                _esp: u32,
                _memory: &impl GuestMemory,
            ) -> Result<u32, ProviderDispatchError> {
                self.static_gl_stub($api)
            }
        )+
    };
}

const GL_VERSION: u32 = 0x0000_1f02;
const GL_EXTENSIONS: u32 = 0x0000_1f03;
const GL_MODELVIEW: u32 = 0x0000_1700;
const GL_PROJECTION: u32 = 0x0000_1701;
const GL_TEXTURE: u32 = 0x0000_1702;
const GL_VERSION_STRING_VA: u32 = PROCESS_DATA_VA + 0x180;
const GL_VERSION_STRING: &[u8] = b"1.1 TRUEOS\0";
const GL_EXTENSIONS_STRING_VA: u32 = PROCESS_DATA_VA + 0x190;
const GL_EXTENSIONS_STRING: &[u8] = b"\0";
const GL_VERTEX_ARRAY: u32 = 0x8074;
const GL_COLOR_ARRAY: u32 = 0x8076;
const GL_FLOAT: u32 = 0x1406;
const GL_UNSIGNED_BYTE: u32 = 0x1401;
const GL_UNSIGNED_SHORT: u32 = 0x1403;
const GL_UNSIGNED_INT: u32 = 0x1405;
const GL_TRIANGLES: u32 = 0x0004;
const GL_COLOR_BUFFER_BIT: u32 = 0x0000_4000;
const GL_DEPTH_BUFFER_BIT: u32 = 0x0000_0100;
const GL_IDENTITY_MATRIX: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

impl XpProcess {
    pub fn gl_context_diagnostic(&self, tid: u32) -> Option<(u32, u32, u32)> {
        self.gl_runtime
            .as_ref()?
            .contexts
            .iter()
            .find_map(|(hglrc, context)| {
                (context.current_tid == Some(tid)).then_some((
                    *hglrc,
                    context.hwnd,
                    context.matrix_mode,
                ))
            })
    }

    fn static_gl_stub(&self, api: &'static str) -> Result<u32, ProviderDispatchError> {
        Err(ProviderDispatchError::Frontier {
            api,
            detail: "static OpenGL entry has no modeled behavior yet".into(),
        })
    }

    static_gl_stubs!(
        gl_disable_static => "glDisable", gl_enable_static => "glEnable",
        gl_lightfv_static => "glLightfv", gl_fogfv_static => "glFogfv",
        gl_fogf_static => "glFogf", gl_fogi_static => "glFogi",
        gl_draw_buffer_static => "glDrawBuffer", gl_depth_func_static => "glDepthFunc",
        gl_alpha_func_static => "glAlphaFunc", gl_blend_func_static => "glBlendFunc",
        gl_tex_envi_static => "glTexEnvi",
        gl_bind_texture_static => "glBindTexture",
        gl_depth_mask_static => "glDepthMask", gl_color_material_static => "glColorMaterial",
        gl_tex_geni_static => "glTexGeni", gl_light_modelfv_static => "glLightModelfv",
        gl_materialfv_static => "glMaterialfv", gl_polygon_offset_static => "glPolygonOffset",
        gl_get_integerv_static => "glGetIntegerv", wgl_get_proc_address_static => "wglGetProcAddress",
        wgl_delete_context_static => "wglDeleteContext", gl_delete_textures_static => "glDeleteTextures",
        gl_tex_sub_image_2d_static => "glTexSubImage2D", gl_tex_image_2d_static => "glTexImage2D",
        gl_pixel_storei_static => "glPixelStorei", gl_tex_parameteri_static => "glTexParameteri",
        gl_gen_textures_static => "glGenTextures", gl_normal_3fv_static => "glNormal3fv",
        gl_normal_pointer_static => "glNormalPointer",
        gl_tex_coord_pointer_static => "glTexCoordPointer",

        gl_scissor_static => "glScissor", gl_depth_range_static => "glDepthRange",

        gl_read_pixels_static => "glReadPixels",
        gl_read_buffer_static => "glReadBuffer",
        gl_lightf_static => "glLightf",
    );

    pub fn bind_gl_ui4_window(&mut self, hwnd: u32, window_id: u32) {
        if let Some(runtime) = self.gl_runtime.as_mut() {
            for context in runtime
                .contexts
                .values_mut()
                .filter(|context| context.hwnd == hwnd)
            {
                context.ui4_window_id = Some(window_id);
            }
        }
    }

    fn gl_context_mut(
        &mut self,
        tid: u32,
        api: &'static str,
    ) -> Result<&mut WglContext, ProviderDispatchError> {
        self.gl_runtime
            .as_mut()
            .and_then(|runtime| {
                runtime
                    .contexts
                    .values_mut()
                    .find(|context| context.current_tid == Some(tid))
            })
            .ok_or_else(|| ProviderDispatchError::Frontier {
                api,
                detail: format!("tid={tid} has no current HGLRC"),
            })
    }

    fn gl_client_state_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
        enabled: bool,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, array] = arguments::<2>(memory, esp)?;
        let api = if enabled {
            "glEnableClientState"
        } else {
            "glDisableClientState"
        };
        let context = self.gl_context_mut(tid, api)?;
        match array {
            GL_VERTEX_ARRAY => context.vertex_array_enabled = enabled,
            GL_COLOR_ARRAY => context.color_array_enabled = enabled,
            _ => {
                return Err(ProviderDispatchError::Frontier {
                    api,
                    detail: format!("array=0x{array:08x} unsupported"),
                });
            }
        }
        Ok(0)
    }

    fn gl_enable_client_state_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        self.gl_client_state_static(tid, esp, memory, true)
    }

    fn gl_disable_client_state_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        self.gl_client_state_static(tid, esp, memory, false)
    }

    fn gl_array_pointer_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
        color: bool,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, size, kind, stride, address] = arguments::<5>(memory, esp)?;
        let api = if color {
            "glColorPointer"
        } else {
            "glVertexPointer"
        };
        let valid = if color {
            matches!(size, 3 | 4) && matches!(kind, GL_FLOAT | GL_UNSIGNED_BYTE)
        } else {
            matches!(size, 2..=4) && kind == GL_FLOAT
        };
        if !valid || stride > 4096 {
            return Err(ProviderDispatchError::Frontier {
                api,
                detail: format!("size={size} type=0x{kind:04x} stride={stride} unsupported"),
            });
        }
        let pointer = GlArrayPointer {
            size,
            kind,
            stride,
            address,
        };
        let context = self.gl_context_mut(tid, api)?;
        if color {
            context.color_pointer = Some(pointer);
        } else {
            context.vertex_pointer = Some(pointer);
        }
        Ok(0)
    }

    fn gl_vertex_pointer_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        self.gl_array_pointer_static(tid, esp, memory, false)
    }

    fn gl_color_pointer_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        self.gl_array_pointer_static(tid, esp, memory, true)
    }

    fn gl_viewport_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, x, y, width, height] = arguments::<5>(memory, esp)?;
        self.gl_context_mut(tid, "glViewport")?.viewport =
            [x as i32, y as i32, width as i32, height as i32];
        Ok(0)
    }

    fn gl_clear_color_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, r, g, b, a] = arguments::<5>(memory, esp)?;
        self.gl_context_mut(tid, "glClearColor")?.clear_color =
            [r, g, b, a].map(|bits| f32::from_bits(bits).clamp(0.0, 1.0));
        Ok(0)
    }

    fn gl_clear_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, mask] = arguments::<2>(memory, esp)?;
        if mask & !(GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT) != 0 {
            return Err(ProviderDispatchError::Frontier {
                api: "glClear",
                detail: format!("mask=0x{mask:08x} unsupported"),
            });
        }
        if mask & GL_COLOR_BUFFER_BIT != 0 {
            let context = self.gl_context_mut(tid, "glClear")?;
            let window_id =
                context
                    .ui4_window_id
                    .ok_or_else(|| ProviderDispatchError::Frontier {
                        api: "glClear",
                        detail: format!("hwnd=0x{:08x} has no UI4 frame", context.hwnd),
                    })?;
            let color = gl_rgba8(context.clear_color);
            let runtime = self.gl_runtime.as_mut().ok_or("GL runtime missing")?;
            let surface = runtime
                .device
                .acquire_ui4_surface(window_id)
                .map_err(|code| ProviderDispatchError::Frontier {
                    api: "glClear",
                    detail: format!("UI4 surface acquire failed window_id={window_id} code={code}"),
                })?;
            let point = runtime
                .device
                .submit_ui4_clear(runtime.queue, surface, color)
                .map_err(|code| ProviderDispatchError::Frontier {
                    api: "glClear",
                    detail: format!("clear submit failed code={code}"),
                })?;
            runtime
                .device
                .wait(runtime.queue, point.value)
                .map_err(|code| ProviderDispatchError::Frontier {
                    api: "glClear",
                    detail: format!("clear wait failed code={code}"),
                })?;
        }
        Ok(0)
    }

    fn gl_draw_elements_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, mode, count, index_kind, indices] = arguments::<5>(memory, esp)?;
        if mode != GL_TRIANGLES
            || count != 3
            || !matches!(index_kind, GL_UNSIGNED_SHORT | GL_UNSIGNED_INT)
        {
            return Err(ProviderDispatchError::Frontier {
                api: "glDrawElements",
                detail: format!(
                    "mode=0x{mode:04x} count={count} index_type=0x{index_kind:04x} unsupported"
                ),
            });
        }
        let context = self.gl_context_mut(tid, "glDrawElements")?;
        if !context.vertex_array_enabled || !context.color_array_enabled {
            return Err(ProviderDispatchError::Frontier {
                api: "glDrawElements",
                detail: "vertex/color arrays must be enabled".into(),
            });
        }
        let vertex_pointer =
            context
                .vertex_pointer
                .ok_or_else(|| ProviderDispatchError::Frontier {
                    api: "glDrawElements",
                    detail: "vertex pointer missing".into(),
                })?;
        let color_pointer =
            context
                .color_pointer
                .ok_or_else(|| ProviderDispatchError::Frontier {
                    api: "glDrawElements",
                    detail: "color pointer missing".into(),
                })?;
        let window_id = context
            .ui4_window_id
            .ok_or_else(|| ProviderDispatchError::Frontier {
                api: "glDrawElements",
                detail: format!("hwnd=0x{:08x} has no UI4 frame", context.hwnd),
            })?;
        let modelview = context.modelview_matrix;
        let projection = context.projection_matrix;
        let clear_rgba8_srgb = gl_rgba8(context.clear_color);
        let mut vertices = [staticgl_triangle::Vertex {
            position: [0.0; 3],
            color: [0.0; 4],
        }; 3];
        for (triangle_index, vertex) in vertices.iter_mut().enumerate() {
            let byte_count = if index_kind == GL_UNSIGNED_SHORT {
                2
            } else {
                4
            };
            let index_address = indices
                .checked_add(triangle_index as u32 * byte_count)
                .ok_or("GL index address overflow")?;
            let index = if index_kind == GL_UNSIGNED_SHORT {
                let mut bytes = [0u8; 2];
                memory.read(index_address, &mut bytes)?;
                u32::from(u16::from_le_bytes(bytes))
            } else {
                read_u32(memory, index_address)?
            };
            let mut point = [0.0, 0.0, 0.0, 1.0];
            for component in 0..vertex_pointer.size {
                point[component as usize] =
                    gl_array_component(memory, vertex_pointer, index, component)?;
            }
            let point = gl_transform(&projection, gl_transform(&modelview, point));
            if point[3] == 0.0 || !point.iter().all(|component| component.is_finite()) {
                return Err(ProviderDispatchError::Frontier {
                    api: "glDrawElements",
                    detail: "nonfinite or zero-w clip position".into(),
                });
            }
            vertex.position = [
                point[0] / point[3],
                point[1] / point[3],
                point[2] / point[3],
            ];
            vertex.color = [1.0; 4];
            for component in 0..color_pointer.size {
                vertex.color[component as usize] =
                    gl_array_component(memory, color_pointer, index, component)?;
            }
        }
        let runtime = self.gl_runtime.as_mut().ok_or("GL runtime missing")?;
        if runtime.triangle_renderer.is_none() {
            runtime.triangle_renderer = Some(
                staticgl_triangle::TriangleRenderer::new(runtime.device).map_err(|code| {
                    ProviderDispatchError::Frontier {
                        api: "glDrawElements",
                        detail: format!("triangle pipeline create failed code={code}"),
                    }
                })?,
            );
        }
        let surface = runtime
            .device
            .acquire_ui4_surface(window_id)
            .map_err(|code| ProviderDispatchError::Frontier {
                api: "glDrawElements",
                detail: format!("UI4 surface acquire failed window_id={window_id} code={code}"),
            })?;
        let point = runtime
            .triangle_renderer
            .as_ref()
            .unwrap()
            .draw(runtime.queue, surface, &vertices, clear_rgba8_srgb)
            .map_err(|code| ProviderDispatchError::Frontier {
                api: "glDrawElements",
                detail: format!("triangle submit failed code={code}"),
            })?;
        runtime
            .device
            .wait(runtime.queue, point.value)
            .map_err(|code| ProviderDispatchError::Frontier {
                api: "glDrawElements",
                detail: format!("triangle wait failed code={code}"),
            })?;
        Ok(0)
    }

    fn wgl_swap_layer_buffers_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, hdc, planes] = arguments::<3>(memory, esp)?;
        let context = self.gl_context_mut(tid, "wglSwapLayerBuffers")?;
        if context.hdc != hdc || planes != 1 {
            return Err(ProviderDispatchError::Frontier {
                api: "wglSwapLayerBuffers",
                detail: format!("tid={tid} hdc=0x{hdc:08x} planes=0x{planes:08x} unsupported"),
            });
        }
        // Each modeled clear/draw already submitted, waited, and published its UI4 frame.
        Ok(1)
    }

    fn gl_finish_static(
        &mut self,
        _esp: u32,
        _memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        // glDrawElements waits for its submitted timeline point before returning.
        Ok(0)
    }

    fn gl_load_matrixf_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, matrix_ptr] = arguments::<2>(memory, esp)?;
        let mut bytes = [0u8; 64];
        memory.read(matrix_ptr, &mut bytes)?;
        let mut matrix = [0f32; 16];
        for (slot, word) in matrix.iter_mut().zip(bytes.chunks_exact(4)) {
            *slot = f32::from_le_bytes(word.try_into().unwrap());
        }
        let runtime = self
            .gl_runtime
            .as_mut()
            .ok_or_else(|| ProviderDispatchError::Frontier {
                api: "glLoadMatrixf",
                detail: format!("tid={tid} has no GL runtime"),
            })?;
        let (_, context) = runtime
            .contexts
            .iter_mut()
            .find(|(_, context)| context.current_tid == Some(tid))
            .ok_or_else(|| ProviderDispatchError::Frontier {
                api: "glLoadMatrixf",
                detail: format!("tid={tid} has no current HGLRC"),
            })?;
        match context.matrix_mode {
            GL_MODELVIEW => context.modelview_matrix = matrix,
            GL_PROJECTION => context.projection_matrix = matrix,
            GL_TEXTURE => context.texture_matrix = matrix,
            mode => {
                return Err(ProviderDispatchError::Frontier {
                    api: "glLoadMatrixf",
                    detail: format!("tid={tid} invalid matrix mode=0x{mode:08x}"),
                });
            }
        }
        Ok(0)
    }

    fn gl_matrix_mode_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, mode] = arguments::<2>(memory, esp)?;
        if !matches!(mode, GL_MODELVIEW | GL_PROJECTION | GL_TEXTURE) {
            return Err(ProviderDispatchError::Frontier {
                api: "glMatrixMode",
                detail: format!("tid={tid} unobserved mode=0x{mode:08x}"),
            });
        }
        let runtime = self
            .gl_runtime
            .as_mut()
            .ok_or_else(|| ProviderDispatchError::Frontier {
                api: "glMatrixMode",
                detail: format!("tid={tid} has no GL runtime"),
            })?;
        let (_, context) = runtime
            .contexts
            .iter_mut()
            .find(|(_, context)| context.current_tid == Some(tid))
            .ok_or_else(|| ProviderDispatchError::Frontier {
                api: "glMatrixMode",
                detail: format!("tid={tid} has no current HGLRC"),
            })?;
        context.matrix_mode = mode;
        Ok(0)
    }

    fn gl_get_string_static(
        &self,
        tid: u32,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, name] = arguments::<2>(memory, esp)?;
        let runtime = self
            .gl_runtime
            .as_ref()
            .ok_or_else(|| ProviderDispatchError::Frontier {
                api: "glGetString",
                detail: format!("tid={tid} has no GL runtime"),
            })?;
        let (hglrc, context) = runtime
            .contexts
            .iter()
            .find(|(_, context)| context.current_tid == Some(tid))
            .ok_or_else(|| ProviderDispatchError::Frontier {
                api: "glGetString",
                detail: format!("tid={tid} has no current HGLRC"),
            })?;

        match name {
            GL_VERSION => {
                memory.write(GL_VERSION_STRING_VA, GL_VERSION_STRING)?;
                Ok(GL_VERSION_STRING_VA)
            }
            GL_EXTENSIONS => {
                memory.write(GL_EXTENSIONS_STRING_VA, GL_EXTENSIONS_STRING)?;
                Ok(GL_EXTENSIONS_STRING_VA)
            }
            _ => Err(ProviderDispatchError::Frontier {
                api: "glGetString",
                detail: format!(
                    "tid={tid} hglrc=0x{hglrc:08x} hwnd=0x{:08x} unobserved name=0x{name:08x}",
                    context.hwnd,
                ),
            }),
        }
    }

    fn wgl_make_current_static(
        &mut self,
        tid: u32,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, hdc, hglrc] = arguments::<3>(memory, esp)?;

        if hglrc == 0 {
            if let Some(runtime) = self.gl_runtime.as_mut() {
                for context in runtime.contexts.values_mut() {
                    if context.current_tid == Some(tid) {
                        context.current_tid = None;
                    }
                }
            }
            return Ok(1);
        }

        let hwnd = match self.gdi_objects.get(&hdc) {
            Some(GdiObject::DeviceContext(DeviceContext {
                target: DcTarget::WindowPaint { hwnd },
                ..
            })) => *hwnd,
            _ => {
                return Err(ProviderDispatchError::Frontier {
                    api: "wglMakeCurrent",
                    detail: format!("tid={tid} hdc=0x{hdc:08x} is not a window DC"),
                });
            }
        };
        let pixel_format = self
            .window_pixel_formats
            .get(&hwnd)
            .copied()
            .ok_or_else(|| ProviderDispatchError::Frontier {
                api: "wglMakeCurrent",
                detail: format!("tid={tid} hwnd=0x{hwnd:08x} has no pixel format"),
            })?;
        let runtime = self
            .gl_runtime
            .as_mut()
            .ok_or_else(|| ProviderDispatchError::Frontier {
                api: "wglMakeCurrent",
                detail: format!("tid={tid} hglrc=0x{hglrc:08x} but GL runtime is absent"),
            })?;
        let target =
            runtime
                .contexts
                .get(&hglrc)
                .ok_or_else(|| ProviderDispatchError::Frontier {
                    api: "wglMakeCurrent",
                    detail: format!("tid={tid} unknown hglrc=0x{hglrc:08x}"),
                })?;
        if target.pixel_format != pixel_format {
            return Err(ProviderDispatchError::Frontier {
                api: "wglMakeCurrent",
                detail: format!(
                    "tid={tid} hglrc=0x{hglrc:08x} context_format={} target_format={} hwnd=0x{hwnd:08x}",
                    target.pixel_format, pixel_format,
                ),
            });
        }
        if let Some(owner_tid) = target.current_tid {
            if owner_tid != tid {
                return Err(ProviderDispatchError::Frontier {
                    api: "wglMakeCurrent",
                    detail: format!(
                        "hglrc=0x{hglrc:08x} already current on tid={owner_tid}, requested_tid={tid}"
                    ),
                });
            }
        }

        for (handle, context) in &mut runtime.contexts {
            if *handle != hglrc && context.current_tid == Some(tid) {
                context.current_tid = None;
            }
        }
        let target = runtime
            .contexts
            .get_mut(&hglrc)
            .expect("validated HGLRC disappeared");
        target.hdc = hdc;
        target.hwnd = hwnd;
        target.current_tid = Some(tid);
        Ok(1)
    }

    fn wgl_create_context_static(
        &mut self,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, hdc] = arguments::<2>(memory, esp)?;
        let hwnd = match self.gdi_objects.get(&hdc) {
            Some(GdiObject::DeviceContext(DeviceContext {
                target: DcTarget::WindowPaint { hwnd },
                ..
            })) => *hwnd,
            _ => {
                return Err(ProviderDispatchError::Frontier {
                    api: "wglCreateContext",
                    detail: format!("hdc=0x{hdc:08x} is not a window DC"),
                });
            }
        };
        let pixel_format = self
            .window_pixel_formats
            .get(&hwnd)
            .copied()
            .ok_or_else(|| ProviderDispatchError::Frontier {
                api: "wglCreateContext",
                detail: format!("hwnd=0x{hwnd:08x} has no pixel format"),
            })?;
        if pixel_format != TRUEOS_GL_PIXEL_FORMAT {
            return Err(ProviderDispatchError::Frontier {
                api: "wglCreateContext",
                detail: format!("hwnd=0x{hwnd:08x} unsupported pixel format={pixel_format}"),
            });
        }

        if self.gl_runtime.is_none() {
            let device = Device::open(Capabilities::DEFAULT.union(Capabilities::PRESENT)).map_err(
                |code| ProviderDispatchError::Frontier {
                    api: "wglCreateContext",
                    detail: format!("vgpu device open failed code={code}"),
                },
            )?;
            let queue = match device.create_queue(QueueClass::Render) {
                Ok(queue) => queue,
                Err(code) => {
                    let _ = device.close();
                    return Err(ProviderDispatchError::Frontier {
                        api: "wglCreateContext",
                        detail: format!("vgpu render queue create failed code={code}"),
                    });
                }
            };
            self.gl_runtime = Some(GlRuntime {
                device,
                queue,
                contexts: HashMap::new(),
                next_context: HGLRC_HANDLE_BASE,
                triangle_renderer: None,
            });
        }

        let runtime = self.gl_runtime.as_mut().ok_or("GL runtime missing")?;
        let hglrc = runtime.next_context;
        runtime.next_context = runtime
            .next_context
            .checked_add(1)
            .ok_or("HGLRC handle overflow")?;
        runtime.contexts.insert(
            hglrc,
            WglContext {
                hdc,
                hwnd,
                pixel_format,
                current_tid: None,
                matrix_mode: GL_MODELVIEW,
                modelview_matrix: GL_IDENTITY_MATRIX,
                projection_matrix: GL_IDENTITY_MATRIX,
                texture_matrix: GL_IDENTITY_MATRIX,
                ui4_window_id: None,
                viewport: [0, 0, 0, 0],
                clear_color: [0.0, 0.0, 0.0, 0.0],
                vertex_array_enabled: false,
                color_array_enabled: false,
                vertex_pointer: None,
                color_pointer: None,
            },
        );
        Ok(hglrc)
    }
}

#[cfg(test)]
mod staticgl_triangle_tests {
    use super::*;

    struct TestMemory(Vec<u8>);
    impl GuestMemory for TestMemory {
        fn read(&self, address: u32, output: &mut [u8]) -> Result<(), &'static str> {
            let start = address as usize;
            output.copy_from_slice(
                self.0
                    .get(start..start + output.len())
                    .ok_or("bad GL read")?,
            );
            Ok(())
        }
        fn write(&mut self, address: u32, input: &[u8]) -> Result<(), &'static str> {
            let start = address as usize;
            self.0
                .get_mut(start..start + input.len())
                .ok_or("bad GL write")?
                .copy_from_slice(input);
            Ok(())
        }
    }

    #[test]
    fn triangle_array_stride_and_color_decode() {
        let mut memory = TestMemory(vec![0; 96]);
        for (index, (x, y, z, color)) in [
            (-0.75f32, -0.5f32, 0.0f32, [255, 0, 0, 255]),
            (0.75, -0.5, 0.0, [0, 255, 0, 255]),
            (0.0, 0.75, 0.0, [0, 0, 255, 255]),
        ]
        .into_iter()
        .enumerate()
        {
            let base = index * 20;
            for (component, value) in [x, y, z].into_iter().enumerate() {
                memory.0[base + component * 4..base + component * 4 + 4]
                    .copy_from_slice(&value.to_le_bytes());
            }
            memory.0[base + 12..base + 16].copy_from_slice(&color);
        }
        let positions = GlArrayPointer {
            size: 3,
            kind: GL_FLOAT,
            stride: 20,
            address: 0,
        };
        let colors = GlArrayPointer {
            size: 4,
            kind: GL_UNSIGNED_BYTE,
            stride: 20,
            address: 12,
        };
        assert_eq!(gl_array_component(&memory, positions, 1, 0).unwrap(), 0.75);
        assert_eq!(gl_array_component(&memory, positions, 2, 1).unwrap(), 0.75);
        assert_eq!(gl_array_component(&memory, colors, 0, 0).unwrap(), 1.0);
        assert_eq!(gl_array_component(&memory, colors, 1, 1).unwrap(), 1.0);
        assert_eq!(gl_array_component(&memory, colors, 2, 2).unwrap(), 1.0);
        assert_eq!(
            gl_transform(&GL_IDENTITY_MATRIX, [0.25, -0.5, 0.0, 1.0]),
            [0.25, -0.5, 0.0, 1.0]
        );
    }
}
