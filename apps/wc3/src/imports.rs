use crate::thunk32::{self, Kind};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LauncherImport {
    pub id: u32,
    pub module: String,
    pub symbol: String,
    pub iat_rva: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum WinCall {
    GetVersion,
    HeapCreate,
    GetVersionExA,
    InitializeCriticalSection,
    EnterCriticalSection,
    LeaveCriticalSection,
    TlsAlloc,
    HeapAlloc,
    HeapFree,
    CreateEventA,
    GetLastError,
    CloseHandle,
    GetTickCount,
    TlsSetValue,
    GetCurrentThreadId,
    GetStartupInfoA,
    GetModuleFileNameA,
    GetModuleHandleA,
    RegisterClassA,
    GetDesktopWindow,
    GetClientRect,
    CreateWindowExA,
    ShowWindow,
    UpdateWindow,
    PeekMessageA,
    SetFocus,
    DefWindowProcA,
    LoadStringA,
    LoadImageA,
    GetObjectA,
    CreateCompatibleDC,
    SelectObject,
    GetDIBColorTable,
    CreatePalette,
    DeleteDC,
    DeleteObject,
    CreateThread,
    ResumeThread,
    GetStdHandle,
    GetFileType,
    SetHandleCount,
    GetCommandLineA,
    GetEnvironmentStringsW,
    GetEnvironmentStringsA,
    FreeEnvironmentStringsA,
    CreateProcessA,
    WaitForMultipleObjects,
    GetACP,
    GetCPInfo,
    GetStringTypeW,
    LCMapStringW,
    MultiByteToWideChar,
    WideCharToMultiByte,
    Unsupported,
}

impl WinCall {
    pub fn from_import(import: &LauncherImport) -> Self {
        let kernel = import.module.eq_ignore_ascii_case("KERNEL32.dll");
        let user = import.module.eq_ignore_ascii_case("USER32.dll");
        match import.symbol.as_str() {
            "GetVersion" if kernel => Self::GetVersion,
            "HeapCreate" if kernel => Self::HeapCreate,
            "GetVersionExA" if kernel => Self::GetVersionExA,
            "InitializeCriticalSection" if kernel => Self::InitializeCriticalSection,
            "EnterCriticalSection" if kernel => Self::EnterCriticalSection,
            "LeaveCriticalSection" if kernel => Self::LeaveCriticalSection,
            "TlsAlloc" if kernel => Self::TlsAlloc,
            "HeapAlloc" if kernel => Self::HeapAlloc,
            "HeapFree" if kernel => Self::HeapFree,
            "CreateEventA" if kernel => Self::CreateEventA,
            "GetLastError" if kernel => Self::GetLastError,
            "CloseHandle" if kernel => Self::CloseHandle,
            "GetTickCount" if kernel => Self::GetTickCount,
            "TlsSetValue" if kernel => Self::TlsSetValue,
            "GetCurrentThreadId" if kernel => Self::GetCurrentThreadId,
            "GetStartupInfoA" if kernel => Self::GetStartupInfoA,
            "GetModuleFileNameA" if kernel => Self::GetModuleFileNameA,
            "GetModuleHandleA" if kernel => Self::GetModuleHandleA,
            "RegisterClassA" if user => Self::RegisterClassA,
            "GetDesktopWindow" if user => Self::GetDesktopWindow,
            "GetClientRect" if user => Self::GetClientRect,
            "CreateWindowExA" if user => Self::CreateWindowExA,
            "ShowWindow" if user => Self::ShowWindow,
            "UpdateWindow" if user => Self::UpdateWindow,
            "PeekMessageA" if user => Self::PeekMessageA,
            "SetFocus" if user => Self::SetFocus,
            "DefWindowProcA" if user => Self::DefWindowProcA,
            "LoadStringA" if user => Self::LoadStringA,
            "LoadImageA" if user => Self::LoadImageA,
            "GetObjectA" if import.module.eq_ignore_ascii_case("GDI32.dll") => Self::GetObjectA,
            "CreateCompatibleDC" if import.module.eq_ignore_ascii_case("GDI32.dll") => {
                Self::CreateCompatibleDC
            }
            "SelectObject" if import.module.eq_ignore_ascii_case("GDI32.dll") => Self::SelectObject,
            "GetDIBColorTable" if import.module.eq_ignore_ascii_case("GDI32.dll") => {
                Self::GetDIBColorTable
            }
            "CreatePalette" if import.module.eq_ignore_ascii_case("GDI32.dll") => {
                Self::CreatePalette
            }
            "DeleteDC" if import.module.eq_ignore_ascii_case("GDI32.dll") => Self::DeleteDC,
            "DeleteObject" if import.module.eq_ignore_ascii_case("GDI32.dll") => Self::DeleteObject,
            "CreateThread" if kernel => Self::CreateThread,
            "ResumeThread" if kernel => Self::ResumeThread,
            "GetStdHandle" if kernel => Self::GetStdHandle,
            "GetFileType" if kernel => Self::GetFileType,
            "SetHandleCount" if kernel => Self::SetHandleCount,
            "GetCommandLineA" if kernel => Self::GetCommandLineA,
            "GetEnvironmentStringsW" if kernel => Self::GetEnvironmentStringsW,
            "GetEnvironmentStrings" if kernel => Self::GetEnvironmentStringsA,
            "FreeEnvironmentStringsA" if kernel => Self::FreeEnvironmentStringsA,
            "CreateProcessA" if kernel => Self::CreateProcessA,
            "WaitForMultipleObjects" if kernel => Self::WaitForMultipleObjects,
            "GetACP" if kernel => Self::GetACP,
            "GetCPInfo" if kernel => Self::GetCPInfo,
            "GetStringTypeW" if kernel => Self::GetStringTypeW,
            "LCMapStringW" if kernel => Self::LCMapStringW,
            "MultiByteToWideChar" if kernel => Self::MultiByteToWideChar,
            "WideCharToMultiByte" if kernel => Self::WideCharToMultiByte,
            _ => Self::Unsupported,
        }
    }

    pub const fn thunk_kind(self) -> Kind {
        match self {
            Self::GetVersion => Kind::Return,
            Self::HeapCreate => Kind::Stdcall(12),
            Self::GetVersionExA
            | Self::InitializeCriticalSection
            | Self::EnterCriticalSection
            | Self::LeaveCriticalSection
            | Self::CloseHandle
            | Self::GetStartupInfoA
            | Self::GetModuleHandleA
            | Self::RegisterClassA
            | Self::UpdateWindow
            | Self::SetFocus
            | Self::DefWindowProcA
            | Self::ResumeThread
            | Self::GetStdHandle
            | Self::GetFileType
            | Self::SetHandleCount
            | Self::FreeEnvironmentStringsA => Kind::Stdcall(4),
            Self::CreateProcessA => Kind::Stdcall(40),
            Self::WaitForMultipleObjects => Kind::Stdcall(16),
            Self::TlsAlloc
            | Self::GetLastError
            | Self::GetTickCount
            | Self::GetCurrentThreadId
            | Self::GetDesktopWindow
            | Self::GetCommandLineA
            | Self::GetEnvironmentStringsW
            | Self::GetEnvironmentStringsA
            | Self::GetACP => Kind::Return,
            Self::HeapAlloc | Self::HeapFree | Self::GetModuleFileNameA => Kind::Stdcall(12),
            Self::CreateEventA | Self::GetStringTypeW | Self::LoadStringA => Kind::Stdcall(16),
            Self::LoadImageA => Kind::Stdcall(24),
            Self::GetObjectA => Kind::Stdcall(12),
            Self::CreateCompatibleDC => Kind::Stdcall(4),
            Self::SelectObject => Kind::Stdcall(8),
            Self::GetDIBColorTable => Kind::Stdcall(16),
            Self::CreatePalette => Kind::Stdcall(4),
            Self::DeleteDC => Kind::Stdcall(4),
            Self::DeleteObject => Kind::Stdcall(4),
            Self::TlsSetValue | Self::GetClientRect | Self::ShowWindow | Self::GetCPInfo => {
                Kind::Stdcall(8)
            }
            Self::CreateWindowExA => Kind::Stdcall(48),
            Self::PeekMessageA => Kind::Stdcall(20),
            Self::CreateThread | Self::LCMapStringW | Self::MultiByteToWideChar => {
                Kind::Stdcall(24)
            }
            Self::WideCharToMultiByte => Kind::Stdcall(32),
            Self::Unsupported => Kind::Stop,
        }
    }
}

pub fn patch(
    image: &mut [u8],
    imports: &[LauncherImport],
    thunks: &mut [u8],
) -> Result<(), &'static str> {
    for import in imports {
        let address = thunk32::address(import.id).ok_or("wc3 thunk address overflow")?;
        let iat = usize::try_from(import.iat_rva).map_err(|_| "wc3 IAT rva")?;
        image
            .get_mut(iat..iat + 4)
            .ok_or("wc3 IAT range")?
            .copy_from_slice(&address.to_le_bytes());
        let offset = usize::try_from(import.id)
            .map_err(|_| "wc3 import id")?
            .checked_mul(thunk32::THUNK_BYTES)
            .ok_or("wc3 thunk offset")?;
        if offset + thunk32::THUNK_BYTES > thunk32::THREAD_EXIT_OFFSET {
            return Err("wc3 imports overlap thread-exit thunk");
        }
        thunk32::write(
            import.id,
            WinCall::from_import(import).thunk_kind(),
            thunks
                .get_mut(offset..offset + thunk32::THUNK_BYTES)
                .ok_or("wc3 thunk output range")?,
        )?;
    }
    Ok(())
}
