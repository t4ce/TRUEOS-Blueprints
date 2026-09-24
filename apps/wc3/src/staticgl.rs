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

impl XpProcess {
    pub fn gl_context_diagnostic(&self, tid: u32) -> Option<(u32, u32, u32)> {
        self.gl_runtime.as_ref()?.contexts.iter().find_map(|(hglrc, context)| {
            (context.current_tid == Some(tid)).then_some((*hglrc, context.hwnd, context.matrix_mode))
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
        gl_enable_client_state_static => "glEnableClientState", gl_tex_envi_static => "glTexEnvi",
        gl_bind_texture_static => "glBindTexture", gl_disable_client_state_static => "glDisableClientState",
        gl_depth_mask_static => "glDepthMask", gl_color_material_static => "glColorMaterial",
        gl_tex_geni_static => "glTexGeni", gl_light_modelfv_static => "glLightModelfv",
        gl_materialfv_static => "glMaterialfv", gl_polygon_offset_static => "glPolygonOffset",
        gl_get_integerv_static => "glGetIntegerv", wgl_get_proc_address_static => "wglGetProcAddress",
        wgl_delete_context_static => "wglDeleteContext", gl_delete_textures_static => "glDeleteTextures",
        gl_tex_sub_image_2d_static => "glTexSubImage2D", gl_tex_image_2d_static => "glTexImage2D",
        gl_pixel_storei_static => "glPixelStorei", gl_tex_parameteri_static => "glTexParameteri",
        gl_gen_textures_static => "glGenTextures", gl_normal_3fv_static => "glNormal3fv",
        gl_normal_pointer_static => "glNormalPointer", gl_vertex_pointer_static => "glVertexPointer",
        gl_color_pointer_static => "glColorPointer", gl_tex_coord_pointer_static => "glTexCoordPointer",
        gl_finish_static => "glFinish", gl_draw_elements_static => "glDrawElements",
        gl_load_matrixf_static => "glLoadMatrixf",
        gl_scissor_static => "glScissor", gl_depth_range_static => "glDepthRange",
        gl_viewport_static => "glViewport", gl_clear_static => "glClear",
        gl_clear_color_static => "glClearColor", gl_read_pixels_static => "glReadPixels",
        gl_read_buffer_static => "glReadBuffer", wgl_swap_layer_buffers_static => "wglSwapLayerBuffers",
        gl_lightf_static => "glLightf",
    );

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
        let runtime = self.gl_runtime.as_mut().ok_or_else(|| ProviderDispatchError::Frontier {
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
        let runtime = self.gl_runtime.as_ref().ok_or_else(|| ProviderDispatchError::Frontier {
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
        let pixel_format = self.window_pixel_formats.get(&hwnd).copied().ok_or_else(|| {
            ProviderDispatchError::Frontier {
                api: "wglMakeCurrent",
                detail: format!("tid={tid} hwnd=0x{hwnd:08x} has no pixel format"),
            }
        })?;
        let runtime = self.gl_runtime.as_mut().ok_or_else(|| {
            ProviderDispatchError::Frontier {
                api: "wglMakeCurrent",
                detail: format!("tid={tid} hglrc=0x{hglrc:08x} but GL runtime is absent"),
            }
        })?;
        let target = runtime.contexts.get(&hglrc).ok_or_else(|| {
            ProviderDispatchError::Frontier {
                api: "wglMakeCurrent",
                detail: format!("tid={tid} unknown hglrc=0x{hglrc:08x}"),
            }
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
        let pixel_format = self.window_pixel_formats.get(&hwnd).copied().ok_or_else(|| {
            ProviderDispatchError::Frontier {
                api: "wglCreateContext",
                detail: format!("hwnd=0x{hwnd:08x} has no pixel format"),
            }
        })?;
        if pixel_format != TRUEOS_GL_PIXEL_FORMAT {
            return Err(ProviderDispatchError::Frontier {
                api: "wglCreateContext",
                detail: format!("hwnd=0x{hwnd:08x} unsupported pixel format={pixel_format}"),
            });
        }

        if self.gl_runtime.is_none() {
            let device = Device::open(Capabilities::DEFAULT.union(Capabilities::PRESENT))
                .map_err(|code| ProviderDispatchError::Frontier {
                    api: "wglCreateContext",
                    detail: format!("vgpu device open failed code={code}"),
                })?;
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
            },
        );
        Ok(hglrc)
    }
}
