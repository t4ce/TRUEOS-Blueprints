use trueos::async_fs::{DirListing, NodeKind};

use crate::{
    pe32::{ImportSymbol, PeImage},
    thunk32,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderSymbol {
    Name(String),
    Ordinal(u16),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderImport {
    pub module: String,
    pub symbol: ProviderSymbol,
    pub iat_rva: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderOp {
    GetEnvironmentStringsW,
    FreeEnvironmentStringsW,
    GetCommandLineA,
    GetStartupInfoA,
    GetStdHandle,
    GetFileType,
    SetHandleCount,
    GetVersion,
    GetVersionExA,
    WideCharToMultiByte,
    HeapCreate,
    HeapAlloc,
    HeapFree,
    InitializeCriticalSection,
    EnterCriticalSection,
    LeaveCriticalSection,
    SetLastError,
    SetUnhandledExceptionFilter,
    VirtualAlloc,
    RegOpenKeyExA,
    CrtMalloc,
    Unknown,
}

impl ProviderOp {
    pub const fn stack_cleanup_bytes(self) -> u8 {
        match self {
            Self::InitializeCriticalSection
            | Self::EnterCriticalSection
            | Self::LeaveCriticalSection
            | Self::SetUnhandledExceptionFilter
            | Self::GetVersionExA
            | Self::FreeEnvironmentStringsW
            | Self::GetStartupInfoA
            | Self::GetStdHandle
            | Self::GetFileType
            | Self::SetHandleCount
            | Self::SetLastError => 4,
            Self::VirtualAlloc => 16,
            Self::WideCharToMultiByte => 32,
            Self::HeapCreate | Self::HeapAlloc | Self::HeapFree => 12,
            Self::RegOpenKeyExA => 20,
            _ => 0,
        }
    }

    pub const fn is_generic_process_local(self) -> bool {
        matches!(
            self,
            Self::FreeEnvironmentStringsW
                | Self::GetStartupInfoA
                | Self::GetStdHandle
                | Self::GetFileType
                | Self::SetHandleCount
        )
    }
}

pub fn provider_op(import: &ProviderImport) -> ProviderOp {
    let ProviderSymbol::Name(symbol) = &import.symbol else {
        return ProviderOp::Unknown;
    };
    if import.module.eq_ignore_ascii_case("KERNEL32.dll") {
        return match symbol.as_str() {
            "GetEnvironmentStringsW" => ProviderOp::GetEnvironmentStringsW,
            "FreeEnvironmentStringsW" => ProviderOp::FreeEnvironmentStringsW,
            "GetCommandLineA" => ProviderOp::GetCommandLineA,
            "GetStartupInfoA" => ProviderOp::GetStartupInfoA,
            "GetStdHandle" => ProviderOp::GetStdHandle,
            "GetFileType" => ProviderOp::GetFileType,
            "SetHandleCount" => ProviderOp::SetHandleCount,
            "GetVersion" => ProviderOp::GetVersion,
            "GetVersionExA" => ProviderOp::GetVersionExA,
            "WideCharToMultiByte" => ProviderOp::WideCharToMultiByte,
            "HeapCreate" => ProviderOp::HeapCreate,
            "HeapAlloc" => ProviderOp::HeapAlloc,
            "HeapFree" => ProviderOp::HeapFree,
            "InitializeCriticalSection" => ProviderOp::InitializeCriticalSection,
            "EnterCriticalSection" => ProviderOp::EnterCriticalSection,
            "LeaveCriticalSection" => ProviderOp::LeaveCriticalSection,
            "SetLastError" => ProviderOp::SetLastError,
            "SetUnhandledExceptionFilter" => ProviderOp::SetUnhandledExceptionFilter,
            "VirtualAlloc" => ProviderOp::VirtualAlloc,
            _ => ProviderOp::Unknown,
        };
    }
    if import.module.eq_ignore_ascii_case("ADVAPI32.dll") && symbol == "RegOpenKeyExA" {
        return ProviderOp::RegOpenKeyExA;
    }
    if import.module.eq_ignore_ascii_case("MSVCRT.dll") && symbol == "malloc" {
        return ProviderOp::CrtMalloc;
    }
    ProviderOp::Unknown
}

pub fn provider_thunk_kind(import: &ProviderImport) -> thunk32::Kind {
    match provider_op(import).stack_cleanup_bytes() {
        0 => thunk32::Kind::Return,
        bytes => thunk32::Kind::Stdcall(bytes),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeModuleRequest {
    pub requested: String,
    pub stored: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChildProvider {
    Native { requested: String, stored: String },
    External { module: String },
}

impl ChildProvider {
    fn requested_module(&self) -> &str {
        match self {
            Self::Native { requested, .. } => requested,
            Self::External { module } => module,
        }
    }
}

pub struct ProviderSurface {
    pub imports: Vec<ProviderImport>,
    pub thunks: Vec<u8>,
    pub external_modules: usize,
    pub named: usize,
    pub ordinal: usize,
    pub native: Vec<NativeModuleRequest>,
    pub providers: Vec<ChildProvider>,
}

pub fn resolve_file(listing: &DirListing, module: &str) -> Result<Option<String>, &'static str> {
    let matches: Vec<_> = listing
        .entries
        .iter()
        .filter(|entry| entry.kind == NodeKind::File && entry.name.eq_ignore_ascii_case(module))
        .collect();
    if matches.len() > 1 {
        return Err("ambiguous case-insensitive child module name");
    }
    Ok(matches.first().map(|entry| entry.name.clone()))
}

pub fn prepare(image: &mut PeImage, listing: &DirListing) -> Result<ProviderSurface, &'static str> {
    if listing.truncated {
        return Err("Warcraft III directory listing truncated");
    }
    let mut providers: Vec<ChildProvider> = Vec::new();
    for import in &image.imports {
        if providers
            .iter()
            .any(|provider| provider.requested_module() == import.module)
        {
            continue;
        }
        providers.push(match resolve_file(listing, &import.module)? {
            Some(stored) => ChildProvider::Native {
                requested: import.module.clone(),
                stored,
            },
            None => ChildProvider::External {
                module: import.module.clone(),
            },
        });
    }

    let mut imports = Vec::new();
    let mut native = Vec::new();
    for provider in &providers {
        let ChildProvider::External { module } = provider else {
            let ChildProvider::Native { requested, stored } = provider else {
                unreachable!()
            };
            native.push(NativeModuleRequest {
                requested: requested.clone(),
                stored: stored.clone(),
            });
            continue;
        };
        for import in image
            .imports
            .iter()
            .filter(|import| import.module == *module)
        {
            imports.push(ProviderImport {
                module: import.module.clone(),
                symbol: match &import.symbol {
                    ImportSymbol::Name(name) => ProviderSymbol::Name(name.clone()),
                    ImportSymbol::Ordinal(ordinal) => ProviderSymbol::Ordinal(*ordinal),
                },
                iat_rva: import.iat_rva,
            });
        }
    }
    let thunk_bytes = imports
        .len()
        .checked_mul(thunk32::THUNK_BYTES)
        .ok_or("child thunk size")?;
    let thunk_len = thunk_bytes.checked_add(0xfff).ok_or("child thunk page")? & !0xfff;
    let mut thunks = vec![0x90; thunk_len];
    for (id, import) in imports.iter().enumerate() {
        let id = u32::try_from(id).map_err(|_| "child thunk id")?;
        let address = thunk32::address(id).ok_or("child thunk address")?;
        let iat = usize::try_from(import.iat_rva).map_err(|_| "child IAT rva")?;
        image
            .image
            .get_mut(iat..iat + 4)
            .ok_or("child IAT range")?
            .copy_from_slice(&address.to_le_bytes());
        let offset = usize::try_from(id).map_err(|_| "child thunk offset")? * thunk32::THUNK_BYTES;
        thunk32::write(
            id,
            provider_thunk_kind(import),
            &mut thunks[offset..offset + thunk32::THUNK_BYTES],
        )?;
    }
    let named = imports
        .iter()
        .filter(|import| matches!(import.symbol, ProviderSymbol::Name(_)))
        .count();
    let ordinal = imports.len() - named;
    let external_modules = providers
        .iter()
        .filter(|provider| matches!(provider, ChildProvider::External { .. }))
        .count();
    Ok(ProviderSurface {
        imports,
        thunks,
        external_modules,
        named,
        ordinal,
        native,
        providers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_resolution_is_ascii_case_insensitive_and_ordinals_survive() {
        let listing = DirListing {
            entries: vec![trueos::async_fs::DirEntry {
                name: "Mss32.dll".into(),
                kind: NodeKind::File,
            }],
            truncated: false,
        };
        let mut image = PeImage {
            image_base: 0x400000,
            entry_rva: 0,
            size_of_image: 0x1000,
            size_of_headers: 0,
            sections: vec![],
            imports: vec![
                crate::pe32::ImportDescriptor {
                    module: "mss32.dll".into(),
                    symbol: ImportSymbol::Name("x".into()),
                    iat_rva: 0,
                },
                crate::pe32::ImportDescriptor {
                    module: "wsock32.dll".into(),
                    symbol: ImportSymbol::Ordinal(25),
                    iat_rva: 4,
                },
            ],
            relocations: vec![],
            exports: vec![],
            image: vec![0; 8],
        };
        let surface = prepare(&mut image, &listing).unwrap();
        assert_eq!(surface.native[0].stored, "Mss32.dll");
        assert_eq!(surface.imports[0].symbol, ProviderSymbol::Ordinal(25));
        assert_eq!(u32::from_le_bytes(image.image[..4].try_into().unwrap()), 0);
        assert_eq!(
            u32::from_le_bytes(image.image[4..8].try_into().unwrap()),
            thunk32::THUNK_BASE
        );
    }

    #[test]
    fn initialize_critical_section_provider_uses_stdcall_cleanup() {
        let import = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("InitializeCriticalSection".into()),
            iat_rva: 0,
        };
        let mut bytes = [0; thunk32::THUNK_BYTES];
        thunk32::write(403, provider_thunk_kind(&import), &mut bytes).unwrap();
        assert_eq!(&bytes[8..11], &[0xc2, 4, 0]);

        let unrelated = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetModuleHandleA".into()),
            iat_rva: 0,
        };
        thunk32::write(404, provider_thunk_kind(&unrelated), &mut bytes).unwrap();
        assert_eq!(bytes[8], 0xc3);
    }

    #[test]
    fn enter_critical_section_provider_uses_stdcall_four() {
        let import = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("EnterCriticalSection".into()),
            iat_rva: 0,
        };
        let mut bytes = [0; thunk32::THUNK_BYTES];
        thunk32::write(435, provider_thunk_kind(&import), &mut bytes).unwrap();
        assert_eq!(&bytes[8..11], &[0xc2, 4, 0]);
    }

    #[test]
    fn leave_critical_section_provider_uses_stdcall_four() {
        let import = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("LeaveCriticalSection".into()),
            iat_rva: 0,
        };
        let mut bytes = [0; thunk32::THUNK_BYTES];
        thunk32::write(437, provider_thunk_kind(&import), &mut bytes).unwrap();
        assert_eq!(&bytes[8..11], &[0xc2, 4, 0]);
    }

    #[test]
    fn set_unhandled_exception_filter_provider_uses_stdcall_four() {
        let import = ProviderImport { module: "KERNEL32.dll".into(), symbol: ProviderSymbol::Name("SetUnhandledExceptionFilter".into()), iat_rva: 0 };
        let mut bytes = [0; thunk32::THUNK_BYTES];
        thunk32::write(409, provider_thunk_kind(&import), &mut bytes).unwrap();
        assert_eq!(&bytes[8..11], &[0xc2, 4, 0]);
    }

    #[test]
    fn virtual_alloc_provider_uses_stdcall_sixteen() {
        let import = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("VirtualAlloc".into()),
            iat_rva: 0,
        };
        let mut bytes = [0; thunk32::THUNK_BYTES];
        thunk32::write(424, provider_thunk_kind(&import), &mut bytes).unwrap();
        assert_eq!(&bytes[8..11], &[0xc2, 0x10, 0]);
    }

    #[test]
    fn get_version_ex_a_provider_uses_stdcall_four() {
        let import = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetVersionExA".into()),
            iat_rva: 0,
        };
        let mut bytes = [0; thunk32::THUNK_BYTES];
        thunk32::write(581, provider_thunk_kind(&import), &mut bytes).unwrap();
        assert_eq!(&bytes[8..11], &[0xc2, 0x04, 0x00]);
    }

    #[test]
    fn free_environment_strings_w_is_pure_process_stdcall_four() {
        let import = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("FreeEnvironmentStringsW".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&import);
        assert_eq!(operation, ProviderOp::FreeEnvironmentStringsW);
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 4);
        let mut bytes = [0; thunk32::THUNK_BYTES];
        thunk32::write(613, provider_thunk_kind(&import), &mut bytes).unwrap();
        assert_eq!(&bytes[8..11], &[0xc2, 0x04, 0x00]);
    }

    #[test]
    fn get_startup_info_a_is_pure_process_stdcall_four() {
        let import = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetStartupInfoA".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&import);
        assert_eq!(operation, ProviderOp::GetStartupInfoA);
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 4);
        let mut bytes = [0; thunk32::THUNK_BYTES];
        thunk32::write(627, provider_thunk_kind(&import), &mut bytes).unwrap();
        assert_eq!(&bytes[8..11], &[0xc2, 0x04, 0x00]);
    }

    #[test]
    fn get_std_handle_is_pure_process_stdcall_four() {
        let import = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetStdHandle".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&import);
        assert_eq!(operation, ProviderOp::GetStdHandle);
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 4);
        let mut bytes = [0; thunk32::THUNK_BYTES];
        thunk32::write(625, provider_thunk_kind(&import), &mut bytes).unwrap();
        assert_eq!(&bytes[8..11], &[0xc2, 0x04, 0x00]);
    }

    #[test]
    fn get_file_type_is_pure_process_stdcall_four() {
        let import = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetFileType".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&import);
        assert_eq!(operation, ProviderOp::GetFileType);
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 4);
        let mut bytes = [0; thunk32::THUNK_BYTES];
        thunk32::write(626, provider_thunk_kind(&import), &mut bytes).unwrap();
        assert_eq!(&bytes[8..11], &[0xc2, 0x04, 0x00]);
    }

    #[test]
    fn set_handle_count_is_pure_process_stdcall_four() {
        let import = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("SetHandleCount".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&import);
        assert_eq!(operation, ProviderOp::SetHandleCount);
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 4);
        let mut bytes = [0; thunk32::THUNK_BYTES];
        thunk32::write(624, provider_thunk_kind(&import), &mut bytes).unwrap();
        assert_eq!(&bytes[8..11], &[0xc2, 0x04, 0x00]);
    }

    #[test]
    fn wide_char_to_multi_byte_provider_uses_stdcall_thirty_two() {
        let import = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("WideCharToMultiByte".into()),
            iat_rva: 0,
        };
        let mut bytes = [0; thunk32::THUNK_BYTES];
        thunk32::write(612, provider_thunk_kind(&import), &mut bytes).unwrap();
        assert_eq!(&bytes[8..11], &[0xc2, 0x20, 0x00]);
    }

    #[test]
    fn heap_providers_use_stdcall_twelve() {
        for (provider_id, symbol) in [(622, "HeapCreate"), (623, "HeapAlloc"), (624, "HeapFree")] {
            let import = ProviderImport {
                module: "KERNEL32.dll".into(),
                symbol: ProviderSymbol::Name(symbol.into()),
                iat_rva: 0,
            };
            let mut bytes = [0; thunk32::THUNK_BYTES];
            thunk32::write(provider_id, provider_thunk_kind(&import), &mut bytes).unwrap();
            assert_eq!(&bytes[8..11], &[0xc2, 0x0c, 0], "{symbol}");
        }
    }

    #[test]
    fn set_last_error_provider_uses_stdcall_four() {
        let import = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("SetLastError".into()),
            iat_rva: 0,
        };
        let mut bytes = [0; thunk32::THUNK_BYTES];
        thunk32::write(408, provider_thunk_kind(&import), &mut bytes).unwrap();
        assert_eq!(&bytes[8..11], &[0xc2, 4, 0]);
    }

    #[test]
    fn malloc_provider_keeps_cdecl_return_cleanup() {
        let import = ProviderImport {
            module: "MSVCRT.dll".into(),
            symbol: ProviderSymbol::Name("malloc".into()),
            iat_rva: 0,
        };
        let mut bytes = [0; thunk32::THUNK_BYTES];
        thunk32::write(337, provider_thunk_kind(&import), &mut bytes).unwrap();
        assert_eq!(bytes[8], 0xc3);
        assert_ne!(&bytes[8..11], &[0xc2, 4, 0]);
    }

    #[test]
    fn initterm_provider_keeps_cdecl_return_cleanup() {
        let import = ProviderImport {
            module: "MSVCRT.dll".into(),
            symbol: ProviderSymbol::Name("_initterm".into()),
            iat_rva: 0,
        };
        let mut bytes = [0; thunk32::THUNK_BYTES];
        thunk32::write(338, provider_thunk_kind(&import), &mut bytes).unwrap();
        assert_eq!(bytes[8], 0xc3);
        assert_ne!(&bytes[8..11], &[0xc2, 8, 0]);
    }

    #[test]
    fn dllonexit_provider_keeps_cdecl_return_cleanup() {
        let import = ProviderImport {
            module: "MSVCRT.dll".into(),
            symbol: ProviderSymbol::Name("__dllonexit".into()),
            iat_rva: 0,
        };
        let mut bytes = [0; thunk32::THUNK_BYTES];
        thunk32::write(342, provider_thunk_kind(&import), &mut bytes).unwrap();
        assert_eq!(bytes[8], 0xc3);
        assert_ne!(&bytes[8..11], &[0xc2, 12, 0]);
    }

    #[test]
    fn reg_open_key_ex_a_provider_uses_stdcall_twenty() {
        let import = ProviderImport {
            module: "ADVAPI32.dll".into(),
            symbol: ProviderSymbol::Name("RegOpenKeyExA".into()),
            iat_rva: 0,
        };
        let mut bytes = [0; thunk32::THUNK_BYTES];
        thunk32::write(548, provider_thunk_kind(&import), &mut bytes).unwrap();
        assert_eq!(&bytes[8..11], &[0xc2, 20, 0]);
    }
}
