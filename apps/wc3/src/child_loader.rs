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

/// External PE data exports are IAT values, not callable provider thunks.
pub fn provider_data_export_address(import: &ProviderImport) -> Option<u32> {
    match (&import.module[..], &import.symbol) {
        (module, ProviderSymbol::Name(symbol))
            if module.eq_ignore_ascii_case("MSVCRT.dll") && symbol == "_acmdln" =>
        {
            Some(crate::process::CRT_ACMDLN_VA)
        }
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderOp {
    GetEnvironmentStringsW,
    FreeEnvironmentStringsW,
    GetCommandLineA,
    GetStartupInfoA,
    GetSystemInfo,
    GetStdHandle,
    GetFileType,
    SetHandleCount,
    GetACP,
    GetCPInfo,
    GetStringTypeW,
    MultiByteToWideChar,
    LCMapStringW,
    GetModuleFileNameA,
    GetModuleHandleA,
    LoadLibraryA,
    GetProcAddress,
    InterlockedExchange,
    TlsAlloc,
    TlsSetValue,
    TlsGetValue,
    GetCurrentProcess,
    GetCurrentProcessId,
    GetCurrentThread,
    GetCurrentThreadId,
    ReadProcessMemory,
    WriteProcessMemory,
    GetLastError,
    CreateFileA,
    GetFileSize,
    SetFilePointer,
    ReadFile,
    WriteFile,
    FlushFileBuffers,
    CreateEventA,
    CreateMutexA,
    ReleaseMutex,
    CloseHandle,
    WaitForSingleObject,
    GetWindowsDirectoryA,
    GetSystemDirectoryA,
    GetTempPathA,
    SetCurrentDirectoryA,
    GetFileAttributesA,
    SetFileAttributesA,
    QueryPerformanceFrequency,
    QueryPerformanceCounter,
    GetLocalTime,
    GetSystemTime,
    GetTimeZoneInformation,
    TimeGetTime,
    GetVersion,
    GetVersionExA,
    WideCharToMultiByte,
    HeapCreate,
    HeapAlloc,
    HeapFree,
    GlobalAlloc,
    MessageBoxA,
    WsprintfA,
    InitializeCriticalSection,
    EnterCriticalSection,
    LeaveCriticalSection,
    SetLastError,
    SetUnhandledExceptionFilter,
    UnhandledExceptionFilter,
    RtlUnwind,
    ExitProcess,
    VirtualAlloc,
    OpenThreadToken,
    OpenProcessToken,
    GetTokenInformation,
    AllocateAndInitializeSid,
    EqualSid,
    RegOpenKeyExA,
    CrtSetAppType,
    CrtGetFmode,
    CrtGetCommode,
    CrtExceptHandler3,
    CrtXcptFilter,
    CrtControlFp,
    CrtGetMainArgs,
    CrtOnExit,
    CrtMalloc,
    CrtStrrchr,
    CrtStrstr,
    CrtFullPath,
    Unknown,
}

impl ProviderOp {
    /// Operations with callable semantics are advertised through dynamic
    /// provider export lookup. Runtime operations can still stop at typed
    /// frontiers for argument shapes whose semantics are not modeled yet.
    pub const fn is_modeled(self) -> bool {
        !matches!(self, Self::Unknown)
    }

    pub const fn stack_cleanup_bytes(self) -> u8 {
        match self {
            Self::InitializeCriticalSection
            | Self::EnterCriticalSection
            | Self::LeaveCriticalSection
            | Self::SetUnhandledExceptionFilter
            | Self::UnhandledExceptionFilter
            | Self::ExitProcess
            | Self::GetVersionExA
            | Self::FreeEnvironmentStringsW
            | Self::GetStartupInfoA
            | Self::GetSystemInfo
            | Self::GetStdHandle
            | Self::GetFileType
            | Self::SetHandleCount
            | Self::SetLastError
            | Self::ReleaseMutex
            | Self::CloseHandle
            | Self::GetModuleHandleA
            | Self::LoadLibraryA
            | Self::QueryPerformanceFrequency
            | Self::QueryPerformanceCounter
            | Self::GetLocalTime
            | Self::GetSystemTime
            | Self::GetTimeZoneInformation
            | Self::SetCurrentDirectoryA
            | Self::GetFileAttributesA
            | Self::FlushFileBuffers => 4,
            Self::GetCPInfo | Self::GetWindowsDirectoryA | Self::GetSystemDirectoryA => 8,
            Self::GetProcAddress
            | Self::WaitForSingleObject
            | Self::GetFileSize
            | Self::GetTempPathA
            | Self::SetFileAttributesA
            | Self::GlobalAlloc
            | Self::InterlockedExchange
            | Self::TlsSetValue => 8,
            Self::TlsGetValue => 4,
            Self::GetStringTypeW
            | Self::RtlUnwind
            | Self::VirtualAlloc
            | Self::CreateEventA
            | Self::OpenThreadToken
            | Self::SetFilePointer
            | Self::MessageBoxA => 16,
            Self::MultiByteToWideChar | Self::LCMapStringW => 24,
            Self::CreateFileA => 28,
            Self::WideCharToMultiByte => 32,
            Self::GetModuleFileNameA | Self::HeapCreate | Self::HeapAlloc | Self::HeapFree
            | Self::CreateMutexA | Self::OpenProcessToken => 12,
            Self::ReadProcessMemory
            | Self::WriteProcessMemory
            | Self::RegOpenKeyExA
            | Self::GetTokenInformation
            | Self::ReadFile
            | Self::WriteFile => 20,
            Self::AllocateAndInitializeSid => 44,
            Self::EqualSid => 8,
            Self::CrtSetAppType
            | Self::CrtGetFmode
            | Self::CrtGetCommode
            | Self::CrtExceptHandler3
            | Self::CrtXcptFilter
            | Self::CrtControlFp
            | Self::CrtGetMainArgs
            | Self::CrtOnExit
            | Self::CrtMalloc
            | Self::CrtStrrchr
            | Self::CrtStrstr
            | Self::CrtFullPath
            | Self::WsprintfA
            | Self::TlsAlloc
            | Self::GetCurrentThreadId => 0,
            _ => 0,
        }
    }

    pub const fn is_generic_process_local(self) -> bool {
        matches!(
            self,
            Self::FreeEnvironmentStringsW
                | Self::GetStartupInfoA
                | Self::GetSystemInfo
                | Self::GetStdHandle
                | Self::GetFileType
                | Self::SetHandleCount
                | Self::GetACP
                | Self::GetCPInfo
                | Self::GetStringTypeW
                | Self::MultiByteToWideChar
                | Self::LCMapStringW
                | Self::GetModuleFileNameA
                | Self::GetModuleHandleA
                | Self::InterlockedExchange
                | Self::TlsAlloc
                | Self::TlsSetValue
                | Self::TlsGetValue
                | Self::GetCurrentProcess
                | Self::GetCurrentProcessId
                | Self::GetCurrentThread
                | Self::GetCurrentThreadId
                | Self::ReadProcessMemory
                | Self::WriteProcessMemory
                | Self::GetLastError
                | Self::CreateFileA
                | Self::GetFileSize
                | Self::SetFilePointer
                | Self::ReadFile
                | Self::WriteFile
                | Self::FlushFileBuffers
                | Self::GetWindowsDirectoryA
                | Self::GetSystemDirectoryA
                | Self::GetTempPathA
                | Self::SetCurrentDirectoryA
                | Self::GetFileAttributesA
                | Self::SetFileAttributesA
                | Self::QueryPerformanceFrequency
                | Self::QueryPerformanceCounter
                | Self::GetLocalTime
                | Self::GetSystemTime
                | Self::GetTimeZoneInformation
                | Self::TimeGetTime
                | Self::OpenThreadToken
                | Self::OpenProcessToken
                | Self::GetTokenInformation
                | Self::AllocateAndInitializeSid
                | Self::EqualSid
                | Self::CrtSetAppType
                | Self::CrtGetFmode
                | Self::CrtGetCommode
                | Self::CrtXcptFilter
                | Self::CrtGetMainArgs
                | Self::CrtOnExit
                | Self::CrtStrrchr
                | Self::CrtStrstr
                | Self::CrtFullPath
                | Self::WsprintfA
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
            "GetSystemInfo" => ProviderOp::GetSystemInfo,
            "GetStdHandle" => ProviderOp::GetStdHandle,
            "GetFileType" => ProviderOp::GetFileType,
            "SetHandleCount" => ProviderOp::SetHandleCount,
            "GetACP" => ProviderOp::GetACP,
            "GetCPInfo" => ProviderOp::GetCPInfo,
            "GetStringTypeW" => ProviderOp::GetStringTypeW,
            "MultiByteToWideChar" => ProviderOp::MultiByteToWideChar,
            "LCMapStringW" => ProviderOp::LCMapStringW,
            "GetModuleFileNameA" => ProviderOp::GetModuleFileNameA,
            "GetModuleHandleA" => ProviderOp::GetModuleHandleA,
            "LoadLibraryA" => ProviderOp::LoadLibraryA,
            "GetProcAddress" => ProviderOp::GetProcAddress,
            "InterlockedExchange" => ProviderOp::InterlockedExchange,
            "TlsAlloc" => ProviderOp::TlsAlloc,
            "TlsSetValue" => ProviderOp::TlsSetValue,
            "TlsGetValue" => ProviderOp::TlsGetValue,
            "GetCurrentProcess" => ProviderOp::GetCurrentProcess,
            "GetCurrentProcessId" => ProviderOp::GetCurrentProcessId,
            "GetCurrentThread" => ProviderOp::GetCurrentThread,
            "GetCurrentThreadId" => ProviderOp::GetCurrentThreadId,
            "ReadProcessMemory" => ProviderOp::ReadProcessMemory,
            "WriteProcessMemory" => ProviderOp::WriteProcessMemory,
            "GetLastError" => ProviderOp::GetLastError,
            "CreateFileA" => ProviderOp::CreateFileA,
            "GetFileSize" => ProviderOp::GetFileSize,
            "SetFilePointer" => ProviderOp::SetFilePointer,
            "ReadFile" => ProviderOp::ReadFile,
            "WriteFile" => ProviderOp::WriteFile,
            "FlushFileBuffers" => ProviderOp::FlushFileBuffers,
            "CreateEventA" => ProviderOp::CreateEventA,
            "CreateMutexA" => ProviderOp::CreateMutexA,
            "ReleaseMutex" => ProviderOp::ReleaseMutex,
            "CloseHandle" => ProviderOp::CloseHandle,
            "WaitForSingleObject" => ProviderOp::WaitForSingleObject,
            "GetWindowsDirectoryA" => ProviderOp::GetWindowsDirectoryA,
            "GetSystemDirectoryA" => ProviderOp::GetSystemDirectoryA,
            "GetTempPathA" => ProviderOp::GetTempPathA,
            "SetCurrentDirectoryA" => ProviderOp::SetCurrentDirectoryA,
            "GetFileAttributesA" => ProviderOp::GetFileAttributesA,
            "SetFileAttributesA" => ProviderOp::SetFileAttributesA,
            "QueryPerformanceFrequency" => ProviderOp::QueryPerformanceFrequency,
            "QueryPerformanceCounter" => ProviderOp::QueryPerformanceCounter,
            "GetLocalTime" => ProviderOp::GetLocalTime,
            "GetSystemTime" => ProviderOp::GetSystemTime,
            "GetTimeZoneInformation" => ProviderOp::GetTimeZoneInformation,
            "GetVersion" => ProviderOp::GetVersion,
            "GetVersionExA" => ProviderOp::GetVersionExA,
            "WideCharToMultiByte" => ProviderOp::WideCharToMultiByte,
            "HeapCreate" => ProviderOp::HeapCreate,
            "HeapAlloc" => ProviderOp::HeapAlloc,
            "HeapFree" => ProviderOp::HeapFree,
            "GlobalAlloc" => ProviderOp::GlobalAlloc,
            "InitializeCriticalSection" => ProviderOp::InitializeCriticalSection,
            "EnterCriticalSection" => ProviderOp::EnterCriticalSection,
            "LeaveCriticalSection" => ProviderOp::LeaveCriticalSection,
            "SetLastError" => ProviderOp::SetLastError,
            "SetUnhandledExceptionFilter" => ProviderOp::SetUnhandledExceptionFilter,
            "UnhandledExceptionFilter" => ProviderOp::UnhandledExceptionFilter,
            "RtlUnwind" => ProviderOp::RtlUnwind,
            "ExitProcess" => ProviderOp::ExitProcess,
            "VirtualAlloc" => ProviderOp::VirtualAlloc,
            _ => ProviderOp::Unknown,
        };
    }
    if import.module.eq_ignore_ascii_case("USER32.dll") {
        return match symbol.as_str() {
            "MessageBoxA" => ProviderOp::MessageBoxA,
            "wsprintfA" => ProviderOp::WsprintfA,
            _ => ProviderOp::Unknown,
        };
    }
    if import.module.eq_ignore_ascii_case("ADVAPI32.dll") {
        return match symbol.as_str() {
            "RegOpenKeyExA" => ProviderOp::RegOpenKeyExA,
            "OpenThreadToken" => ProviderOp::OpenThreadToken,
            "OpenProcessToken" => ProviderOp::OpenProcessToken,
            "GetTokenInformation" => ProviderOp::GetTokenInformation,
            "AllocateAndInitializeSid" => ProviderOp::AllocateAndInitializeSid,
            "EqualSid" => ProviderOp::EqualSid,
            _ => ProviderOp::Unknown,
        };
    }
    if import.module.eq_ignore_ascii_case("WINMM.dll") && symbol == "timeGetTime" {
        return ProviderOp::TimeGetTime;
    }
    if import.module.eq_ignore_ascii_case("MSVCRT.dll") {
        return match symbol.as_str() {
            "__set_app_type" => ProviderOp::CrtSetAppType,
            "__p__fmode" => ProviderOp::CrtGetFmode,
            "__p__commode" => ProviderOp::CrtGetCommode,
            "_except_handler3" => ProviderOp::CrtExceptHandler3,
            "_XcptFilter" => ProviderOp::CrtXcptFilter,
            "_controlfp" => ProviderOp::CrtControlFp,
            "__getmainargs" => ProviderOp::CrtGetMainArgs,
            "_onexit" => ProviderOp::CrtOnExit,
            "malloc" => ProviderOp::CrtMalloc,
            "strrchr" => ProviderOp::CrtStrrchr,
            "strstr" => ProviderOp::CrtStrstr,
            "_fullpath" => ProviderOp::CrtFullPath,
            _ => ProviderOp::Unknown,
        };
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
        let iat = usize::try_from(import.iat_rva).map_err(|_| "child IAT rva")?;
        let address = provider_data_export_address(import)
            .or_else(|| thunk32::address(id))
            .ok_or("child provider address")?;
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
crate::wc3_child_loader_tests_1!();
