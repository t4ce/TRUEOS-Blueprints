// WC3 CHILD LOADLIBRARY LOCAL IMPORT source=trueosfs index=729 module="GDI32.dll" symbol=Name("SetTextColor") iat_rva=0x00706044
// WC3 CHILD LOADLIBRARY LOCAL IMPORT source=trueosfs index=728 module="GDI32.dll" symbol=Name("SetBkColor") iat_rva=0x00706040
// WC3 CHILD LOADLIBRARY LOCAL IMPORT source=trueosfs index=727 module="GDI32.dll" symbol=Name("GetDeviceCaps") iat_rva=0x0070603c
// WC3 CHILD LOADLIBRARY LOCAL IMPORT source=trueosfs index=726 module="GDI32.dll" symbol=Name("SetPixelFormat") iat_rva=0x00706038
// WC3 CHILD LOADLIBRARY LOCAL IMPORT source=trueosfs index=725 module="GDI32.dll" symbol=Name("TextOutW") iat_rva=0x00706034
// WC3 CHILD LOADLIBRARY LOCAL IMPORT source=trueosfs index=724 module="GDI32.dll" symbol=Name("SetDeviceGammaRamp") iat_rva=0x00706030
// WC3 CHILD LOADLIBRARY LOCAL IMPORT source=trueosfs index=723 module="GDI32.dll" symbol=Name("DescribePixelFormat") iat_rva=0x0070602c
// WC3 CHILD LOADLIBRARY LOCAL IMPORT source=trueosfs index=722 module="GDI32.dll" symbol=Name("ChoosePixelFormat") iat_rva=0x00706028
// WC3 CHILD LOADLIBRARY LOCAL IMPORT source=trueosfs index=721 module="GDI32.dll" symbol=Name("SetTextAlign") iat_rva=0x00706024
// WC3 CHILD LOADLIBRARY LOCAL IMPORT source=trueosfs index=720 module="GDI32.dll" symbol=Name("SelectObject") iat_rva=0x00706020
// WC3 CHILD LOADLIBRARY LOCAL IMPORT source=trueosfs index=719 module="GDI32.dll" symbol=Name("GetDeviceGammaRamp") iat_rva=0x0070601c
// WC3 CHILD LOADLIBRARY LOCAL IMPORT source=trueosfs index=718 module="GDI32.dll" symbol=Name("CreateFontA") iat_rva=0x00706018
// WC3 CHILD LOADLIBRARY LOCAL IMPORT source=trueosfs index=717 module="GDI32.dll" symbol=Name("GetStockObject") iat_rva=0x00706014
// WC3 CHILD LOADLIBRARY LOCAL IMPORT source=trueosfs index=716 module="GDI32.dll" symbol=Name("DeleteObject") iat_rva=0x00706010

impl XpProcess {
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
