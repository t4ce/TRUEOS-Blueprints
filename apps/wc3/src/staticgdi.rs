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
        &self,
        _esp: u32,
        _memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        self.static_gdi_stub("SetPixelFormat")
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
        _esp: u32,
        _memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        self.static_gdi_stub("DescribePixelFormat")
    }

    fn choose_pixel_format_static(
        &self,
        _esp: u32,
        _memory: &impl GuestMemory,
    ) -> Result<u32, ProviderDispatchError> {
        self.static_gdi_stub("ChoosePixelFormat")
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
