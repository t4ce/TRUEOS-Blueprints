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

pub fn provider_thunk_kind(import: &ProviderImport) -> thunk32::Kind {
    match &import.symbol {
        ProviderSymbol::Name(symbol)
            if import.module.eq_ignore_ascii_case("KERNEL32.dll")
                && symbol == "InitializeCriticalSection" =>
        {
            thunk32::Kind::Stdcall(4)
        }
        ProviderSymbol::Name(symbol)
            if import.module.eq_ignore_ascii_case("KERNEL32.dll") && symbol == "SetLastError" =>
        {
            thunk32::Kind::Stdcall(4)
        }
        ProviderSymbol::Name(symbol)
            if import.module.eq_ignore_ascii_case("ADVAPI32.dll") && symbol == "RegOpenKeyExA" =>
        {
            thunk32::Kind::Stdcall(20)
        }
        _ => thunk32::Kind::Return,
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
