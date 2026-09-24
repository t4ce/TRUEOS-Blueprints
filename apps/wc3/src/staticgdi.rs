/* GDI32.dll
SetTextColor         iat_rva=0x00706044
SetBkColor           iat_rva=0x00706040
GetDeviceCaps        iat_rva=0x0070603c
SetPixelFormat       iat_rva=0x00706038
TextOutW             iat_rva=0x00706034
SetDeviceGammaRamp   iat_rva=0x00706030
DescribePixelFormat  iat_rva=0x0070602c
ChoosePixelFormat    iat_rva=0x00706028
SetTextAlign         iat_rva=0x00706024
SelectObject         iat_rva=0x00706020
GetDeviceGammaRamp   iat_rva=0x0070601c
CreateFontA          iat_rva=0x00706018
GetStockObject       iat_rva=0x00706014
DeleteObject         iat_rva=0x00706010
*/

const TRUEOS_GL_PIXEL_FORMAT: u32 = 1;
const PIXELFORMATDESCRIPTOR_BYTES: usize = 40;

fn trueos_gl_pixel_format_descriptor(copied_bytes: u16) -> [u8; PIXELFORMATDESCRIPTOR_BYTES] {
    const PFD_DOUBLEBUFFER: u32 = 0x0000_0001;
    const PFD_DRAW_TO_WINDOW: u32 = 0x0000_0004;
    const PFD_SUPPORT_OPENGL: u32 = 0x0000_0020;

    let mut pfd = [0u8; PIXELFORMATDESCRIPTOR_BYTES];
    pfd[0..2].copy_from_slice(&copied_bytes.to_le_bytes());
    pfd[2..4].copy_from_slice(&1u16.to_le_bytes());
    pfd[4..8].copy_from_slice(
        &(PFD_DOUBLEBUFFER | PFD_DRAW_TO_WINDOW | PFD_SUPPORT_OPENGL).to_le_bytes(),
    );
    pfd[8] = 0;
    pfd[9] = 32;
    pfd[10] = 8;
    pfd[11] = 16;
    pfd[12] = 8;
    pfd[13] = 8;
    pfd[14] = 8;
    pfd[15] = 0;
    pfd[16] = 8;
    pfd[17] = 24;
    pfd[23] = 24;
    pfd[26] = 0;
    pfd
}

impl XpProcess {
    fn static_gdi_stub(&self, api: &'static str) -> Result<u32, ProviderDispatchError> {
        Err(ProviderDispatchError::Frontier {
            api,
            detail: "static GDI entry has no modeled behavior yet".into(),
        })
    }

    fn set_text_color_static(
        &self,
        _esp: u32,
        _memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        self.static_gdi_stub("SetTextColor")
    }

    fn set_bk_color_static(
        &self,
        _esp: u32,
        _memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        self.static_gdi_stub("SetBkColor")
    }

    fn set_pixel_format_static(
        &mut self,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, hdc, format, pfd] = arguments::<4>(memory, esp)?;
        let hwnd = match self.gdi_objects.get(&hdc) {
            Some(GdiObject::DeviceContext(DeviceContext {
                target: DcTarget::WindowPaint { hwnd },
                ..
            })) => *hwnd,
            _ => {
                return Err(ProviderDispatchError::Frontier {
                    api: "SetPixelFormat",
                    detail: format!("hdc=0x{hdc:08x} is not a window DC"),
                });
            }
        };

        if format != TRUEOS_GL_PIXEL_FORMAT {
            return Err(ProviderDispatchError::Frontier {
                api: "SetPixelFormat",
                detail: format!("hwnd=0x{hwnd:08x} unsupported format={format}"),
            });
        }
        if pfd == 0 {
            return Err(ProviderDispatchError::Frontier {
                api: "SetPixelFormat",
                detail: format!("hwnd=0x{hwnd:08x} format={format} null ppfd"),
            });
        }

        if let Some(existing) = self.window_pixel_formats.get(&hwnd).copied() {
            if existing == format {
                return Ok(0);
            }
            return Err(ProviderDispatchError::Frontier {
                api: "SetPixelFormat",
                detail: format!(
                    "hwnd=0x{hwnd:08x} already has format={existing}, requested={format}"
                ),
            });
        }

        self.window_pixel_formats.insert(hwnd, format);
        Ok(1)
    }

    fn text_out_w_static(
        &self,
        _esp: u32,
        _memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        self.static_gdi_stub("TextOutW")
    }

    fn set_device_gamma_ramp_static(
        &self,
        _esp: u32,
        _memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        self.static_gdi_stub("SetDeviceGammaRamp")
    }

    fn describe_pixel_format_static(
        &self,
        esp: u32,
        memory: &mut impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, hdc, format, nbytes, output] = arguments::<5>(memory, esp)?;
        let Some(GdiObject::DeviceContext(DeviceContext {
            target: DcTarget::WindowPaint { .. },
            ..
        })) = self.gdi_objects.get(&hdc)
        else {
            return Err(ProviderDispatchError::Frontier {
                api: "DescribePixelFormat",
                detail: format!("hdc=0x{hdc:08x} is not a window DC"),
            });
        };

        if format != TRUEOS_GL_PIXEL_FORMAT {
            return Err(ProviderDispatchError::Frontier {
                api: "DescribePixelFormat",
                detail: format!("unobserved format index={format}"),
            });
        }

        if output != 0 {
            let copied = nbytes.min(PIXELFORMATDESCRIPTOR_BYTES as u32) as usize;
            let pfd = trueos_gl_pixel_format_descriptor(copied as u16);
            memory.write(output, &pfd[..copied])?;
        }

        Ok(TRUEOS_GL_PIXEL_FORMAT)
    }

    fn choose_pixel_format_static(
        &self,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        const PFD_DOUBLEBUFFER: u32 = 0x0000_0001;
        const PFD_DRAW_TO_WINDOW: u32 = 0x0000_0004;
        const PFD_SUPPORT_OPENGL: u32 = 0x0000_0020;
        const PFD_TYPE_RGBA: u8 = 0;
        const PFD_MAIN_PLANE: u8 = 0;

        let [_, hdc, pfd] = arguments::<3>(memory, esp)?;
        let Some(GdiObject::DeviceContext(DeviceContext {
            target: DcTarget::WindowPaint { .. },
            ..
        })) = self.gdi_objects.get(&hdc)
        else {
            return Err(ProviderDispatchError::Frontier {
                api: "ChoosePixelFormat",
                detail: format!("hdc=0x{hdc:08x} is not a window DC"),
            });
        };
        if pfd == 0 {
            return Ok(0);
        }

        let size = read_u16(memory, pfd)?;
        let version = read_u16(memory, pfd + 2)?;
        let flags = read_u32(memory, pfd + 4)?;
        let mut fields = [0u8; 20];
        memory.read(pfd + 8, &mut fields)?;
        let pixel_type = fields[0];
        let color_bits = fields[1];
        let depth_bits = fields[15];
        let stencil_bits = fields[16];
        let aux_buffers = fields[17];
        let layer_type = fields[18];
        let required_flags = PFD_DOUBLEBUFFER | PFD_DRAW_TO_WINDOW | PFD_SUPPORT_OPENGL;

        if size != 40
            || version != 1
            || flags & required_flags != required_flags
            || pixel_type != PFD_TYPE_RGBA
            || color_bits > 32
            || depth_bits > 24
            || stencil_bits != 0
            || aux_buffers != 0
            || layer_type != PFD_MAIN_PLANE
        {
            return Err(ProviderDispatchError::Frontier {
                api: "ChoosePixelFormat",
                detail: format!(
                    "unsupported PFD size={size} version={version} flags=0x{flags:08x} type={pixel_type} color={color_bits} depth={depth_bits} stencil={stencil_bits} aux={aux_buffers} layer={layer_type}"
                ),
            });
        }

        Ok(TRUEOS_GL_PIXEL_FORMAT)
    }

    fn set_text_align_static(
        &self,
        _esp: u32,
        _memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        self.static_gdi_stub("SetTextAlign")
    }

    fn select_object_static(
        &self,
        _esp: u32,
        _memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        self.static_gdi_stub("SelectObject")
    }

    fn get_device_gamma_ramp_static(
        &self,
        _esp: u32,
        _memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        self.static_gdi_stub("GetDeviceGammaRamp")
    }

    fn create_font_a_static(
        &self,
        _esp: u32,
        _memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        self.static_gdi_stub("CreateFontA")
    }

    fn get_stock_object_static(
        &self,
        _esp: u32,
        _memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        self.static_gdi_stub("GetStockObject")
    }

    fn delete_object_static(
        &self,
        _esp: u32,
        _memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        self.static_gdi_stub("DeleteObject")
    }

    fn get_device_caps(
        &self,
        esp: u32,
        memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        let [_, hdc, index] = arguments::<3>(memory, esp)?;
        if !matches!(
            self.gdi_objects.get(&hdc),
            Some(GdiObject::DeviceContext(DeviceContext {
                target: DcTarget::WindowPaint { .. },
                ..
            }))
        ) {
            return Err(ProviderDispatchError::Frontier {
                api: "GetDeviceCaps",
                detail: format!("hdc=0x{hdc:08x} is not a window DC"),
            });
        }
        let (width, height) = self.desktop_size();
        let value = match index {
            2 => 2,
            8 => width,
            10 => height,
            12 => 32,
            14 => 1,
            24 => u32::MAX,
            38 => 0x0000_0081,
            88 | 90 => 96,
            104 | 106 => 0,
            108 => 24,
            116 => 60,
            117 => height,
            118 => width,
            _ => {
                return Err(ProviderDispatchError::Frontier {
                    api: "GetDeviceCaps",
                    detail: format!("hdc=0x{hdc:08x} index={index}"),
                });
            }
        };
        Ok(value)
    }
}
