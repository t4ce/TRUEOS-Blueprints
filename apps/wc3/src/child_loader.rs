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
    GlobalMemoryStatus,
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
    FreeLibrary,
    GetProcAddress,
    InterlockedExchange,
    InterlockedIncrement,
    InterlockedDecrement,
    TlsAlloc,
    TlsSetValue,
    TlsGetValue,
    GetCurrentProcess,
    GetCurrentProcessId,
    GetProcessHeap,
    GetCurrentThread,
    GetCurrentThreadId,
    ReadProcessMemory,
    WriteProcessMemory,
    GetLastError,
    FormatMessageA,
    GetTickCount,
    Sleep,
    CreateThread,
    SetThreadPriority,
    GetThreadPriority,
    CreateFileA,
    GetFileSize,
    SetFilePointer,
    ReadFile,
    WriteFile,
    FlushFileBuffers,
    CreateEventA,
    OpenEventA,
    SetEvent,
    ResetEvent,
    CreateMutexA,
    ReleaseMutex,
    CloseHandle,
    WaitForSingleObject,
    WaitForMultipleObjects,
    GetWindowsDirectoryA,
    GetSystemDirectoryA,
    GetTempPathA,
    GetDriveTypeA,
    GetVolumeInformationA,
    GetDiskFreeSpaceA,
    SetCurrentDirectoryA,
    GetFileAttributesA,
    SetFileAttributesA,
    FindFirstFileA,
    FindClose,
    QueryPerformanceFrequency,
    QueryPerformanceCounter,
    GetLocalTime,
    GetSystemTime,
    SystemTimeToFileTime,
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
    LoadStringA,
    WsprintfA,
    InitializeCriticalSection,
    EnterCriticalSection,
    LeaveCriticalSection,
    SetLastError,
    DisableThreadLibraryCalls,
    SetUnhandledExceptionFilter,
    UnhandledExceptionFilter,
    RtlUnwind,
    ExitProcess,
    VirtualAlloc,
    VirtualFree,
    OpenThreadToken,
    OpenProcessToken,
    GetTokenInformation,
    AllocateAndInitializeSid,
    EqualSid,
    RegOpenKeyExA,
    RegQueryValueExA,
    RegCloseKey,
    CrtSetAppType,
    CrtGetFmode,
    CrtGetCommode,
    CrtExceptHandler3,
    CrtXcptFilter,
    CrtControlFp,
    CrtControl87,
    CrtClearFp,
    CrtGetMainArgs,
    CrtOnExit,
    CrtVsnprintf,
    CrtMalloc,
    CrtMemmove,
    CrtIsDigit,
    CrtToUpper,
    CrtAtol,
    CrtSscanf,
    CrtFtol,
    CrtRand,
    CrtSrand,
    CrtStrncpy,
    CrtStrpbrk,
    CrtStrlwr,
    CrtStrupr,
    CrtStrncmp,
    CrtStricmp,
    CrtStrrchr,
    CrtStrstr,
    CrtStrnicmp,
    CrtFullPath,
    CrtBeginThreadEx,
    Direct3DCreate8,
    D3D8Release,
    D3D8GetAdapterIdentifier,
    EnumDisplayDevicesA,
    EnumDisplaySettingsA,
    ChangeDisplaySettingsExA,
    SetWindowPos,
    ShowWindow,
    UpdateWindow,
    SetFocus,
    SetForegroundWindow,
    GetDesktopWindow,
    GetWindowRect,
    ImmAssociateContext,
    SetWindowTextA,
    ClipCursor,
    BeginPaint,
    FillRect,
    EndPaint,
    GetDC,
    ReleaseDC,
    GetDeviceCaps,
    WglMakeCurrent,
    GlDisable,
    GlEnable,
    GlLightfv,
    GlFogfv,
    GlFogf,
    GlFogi,
    GlDrawBuffer,
    GlDepthFunc,
    GlAlphaFunc,
    GlBlendFunc,
    GlEnableClientState,
    GlTexEnvi,
    GlBindTexture,
    GlDisableClientState,
    GlDepthMask,
    GlColorMaterial,
    GlTexGeni,
    GlLightModelfv,
    GlMaterialfv,
    GlPolygonOffset,
    GlGetIntegerv,
    WglGetProcAddress,
    GlGetString,
    WglCreateContext,
    WglDeleteContext,
    GlDeleteTextures,
    GlTexSubImage2D,
    GlTexImage2D,
    GlPixelStorei,
    GlTexParameteri,
    GlGenTextures,
    GlNormal3fv,
    GlNormalPointer,
    GlVertexPointer,
    GlColorPointer,
    GlTexCoordPointer,
    GlFinish,
    GlDrawElements,
    GlLoadMatrixf,
    GlMatrixMode,
    GlScissor,
    GlDepthRange,
    GlViewport,
    GlClear,
    GlClearColor,
    GlReadPixels,
    GlReadBuffer,
    WglSwapLayerBuffers,
    GlLightf,
    SetTextColor,
    SetBkColor,
    SetPixelFormat,
    TextOutW,
    SetDeviceGammaRamp,
    DescribePixelFormat,
    ChoosePixelFormat,
    SetTextAlign,
    SelectObject,
    GetDeviceGammaRamp,
    CreateFontA,
    GetStockObject,
    DeleteObject,
    LoadImageA,
    LoadCursorA,
    RegisterClassA,
    RegisterClassExA,
    CreateWindowExA,
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
            | Self::GlobalMemoryStatus
            | Self::GetStdHandle
            | Self::GetFileType
            | Self::SetHandleCount
            | Self::SetLastError
            | Self::DisableThreadLibraryCalls
            | Self::ReleaseMutex
            | Self::CloseHandle
            | Self::GetModuleHandleA
            | Self::LoadLibraryA
            | Self::FreeLibrary
            | Self::QueryPerformanceFrequency
            | Self::QueryPerformanceCounter
            | Self::GetLocalTime
            | Self::GetSystemTime
            | Self::GetTimeZoneInformation
            | Self::SetCurrentDirectoryA
            | Self::GetFileAttributesA
            | Self::FindClose
            | Self::FlushFileBuffers
            | Self::Sleep
            | Self::GetDriveTypeA
            | Self::RegCloseKey
            | Self::Direct3DCreate8
            | Self::D3D8Release => 4,
            Self::GetVolumeInformationA => 32,
            Self::SetEvent | Self::ResetEvent => 4,
            Self::GetCPInfo
            | Self::GetWindowsDirectoryA
            | Self::GetSystemDirectoryA
            | Self::SystemTimeToFileTime => 8,
            Self::GetProcAddress
            | Self::WaitForSingleObject
            | Self::GetFileSize
            | Self::GetTempPathA
            | Self::SetFileAttributesA
            | Self::FindFirstFileA
            | Self::GlobalAlloc
            | Self::InterlockedExchange
            | Self::TlsSetValue => 8,
            Self::WaitForMultipleObjects => 16,
            Self::TlsGetValue | Self::InterlockedIncrement | Self::InterlockedDecrement => 4,
            Self::GetStringTypeW
            | Self::RtlUnwind
            | Self::VirtualAlloc
            | Self::CreateEventA
            | Self::OpenThreadToken
            | Self::SetFilePointer
            | Self::MessageBoxA
            | Self::LoadStringA => 16,
            Self::OpenEventA => 12,
            Self::D3D8GetAdapterIdentifier | Self::EnumDisplayDevicesA => 16,
            Self::EnumDisplaySettingsA => 12,
            Self::ChangeDisplaySettingsExA => 20,
            Self::SetWindowPos => 28,
            Self::ShowWindow => 8,
            Self::UpdateWindow => 4,
            Self::SetFocus => 4,
            Self::SetForegroundWindow => 4,
            Self::GetDesktopWindow => 0,
            Self::GetWindowRect => 8,
            Self::ImmAssociateContext => 8,
            Self::SetWindowTextA => 8,
            Self::ClipCursor => 4,
            Self::BeginPaint | Self::EndPaint => 8,
            Self::FillRect => 12,
            Self::GetDC => 4,
            Self::ReleaseDC => 8,
            Self::GetDeviceCaps => 8,
            Self::WglMakeCurrent => 8,
            Self::GlDisable
            | Self::GlEnable
            | Self::GlDrawBuffer
            | Self::GlDepthFunc
            | Self::GlEnableClientState
            | Self::GlDisableClientState
            | Self::GlDepthMask
            | Self::GlGetString
            | Self::WglGetProcAddress
            | Self::WglCreateContext
            | Self::WglDeleteContext
            | Self::GlNormal3fv
            | Self::GlLoadMatrixf
            | Self::GlMatrixMode
            | Self::GlClear
            | Self::GlReadBuffer => 4,
            Self::GlLightfv
            | Self::GlTexEnvi
            | Self::GlTexGeni
            | Self::GlMaterialfv
            | Self::GlTexParameteri
            | Self::GlNormalPointer
            | Self::GlLightf => 12,
            Self::GlFogfv
            | Self::GlFogf
            | Self::GlFogi
            | Self::GlAlphaFunc
            | Self::GlBlendFunc
            | Self::GlBindTexture
            | Self::GlColorMaterial
            | Self::GlLightModelfv
            | Self::GlPolygonOffset
            | Self::GlGetIntegerv
            | Self::GlDeleteTextures
            | Self::GlPixelStorei
            | Self::GlGenTextures
            | Self::WglSwapLayerBuffers => 8,
            Self::GlTexSubImage2D | Self::GlTexImage2D => 36,
            Self::GlVertexPointer
            | Self::GlColorPointer
            | Self::GlTexCoordPointer
            | Self::GlDrawElements
            | Self::GlDepthRange
            | Self::GlScissor
            | Self::GlViewport
            | Self::GlClearColor => 16,
            Self::GlFinish => 0,
            Self::GlReadPixels => 28,
            Self::SetTextColor | Self::SetBkColor | Self::SetTextAlign => 8,
            Self::SetPixelFormat => 12,
            Self::TextOutW => 20,
            Self::SetDeviceGammaRamp | Self::GetDeviceGammaRamp => 8,
            Self::DescribePixelFormat => 16,
            Self::ChoosePixelFormat => 8,
            Self::SelectObject => 8,
            Self::DeleteObject | Self::GetStockObject => 4,
            Self::CreateFontA => 56,
            Self::LoadImageA => 24,
            Self::LoadCursorA => 8,
            Self::RegisterClassA | Self::RegisterClassExA => 4,
            Self::CreateWindowExA => 48,
            Self::MultiByteToWideChar | Self::LCMapStringW | Self::RegQueryValueExA => 24,
            Self::CreateThread => 24,
            Self::SetThreadPriority => 8,
            Self::GetThreadPriority => 4,
            Self::CreateFileA | Self::FormatMessageA => 28,
            Self::WideCharToMultiByte => 32,
            Self::GetModuleFileNameA
            | Self::HeapCreate
            | Self::HeapAlloc
            | Self::HeapFree
            | Self::CreateMutexA
            | Self::OpenProcessToken
            | Self::VirtualFree => 12,
            Self::ReadProcessMemory
            | Self::WriteProcessMemory
            | Self::RegOpenKeyExA
            | Self::GetDiskFreeSpaceA
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
            | Self::CrtControl87
            | Self::CrtClearFp
            | Self::CrtGetMainArgs
            | Self::CrtOnExit
            | Self::CrtVsnprintf
            | Self::CrtMalloc
            | Self::CrtMemmove
            | Self::CrtIsDigit
            | Self::CrtToUpper
            | Self::CrtAtol
            | Self::CrtSscanf
            | Self::CrtFtol
            | Self::CrtRand
            | Self::CrtSrand
            | Self::CrtStrncpy
            | Self::CrtStrpbrk
            | Self::CrtStrlwr
            | Self::CrtStrupr
            | Self::CrtStrncmp
            | Self::CrtStricmp
            | Self::CrtStrrchr
            | Self::CrtStrstr
            | Self::CrtStrnicmp
            | Self::CrtFullPath
            | Self::CrtBeginThreadEx
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
                | Self::GlobalMemoryStatus
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
                | Self::InterlockedIncrement
                | Self::InterlockedDecrement
                | Self::TlsAlloc
                | Self::TlsSetValue
                | Self::TlsGetValue
                | Self::GetCurrentProcess
                | Self::GetCurrentProcessId
                | Self::GetProcessHeap
                | Self::GetCurrentThread
                | Self::GetCurrentThreadId
                | Self::ReadProcessMemory
                | Self::WriteProcessMemory
                | Self::GetLastError
                | Self::FormatMessageA
                | Self::GetTickCount
                | Self::Sleep
                | Self::DisableThreadLibraryCalls
                | Self::CreateFileA
                | Self::GetFileSize
                | Self::SetFilePointer
                | Self::ReadFile
                | Self::WriteFile
                | Self::FlushFileBuffers
                | Self::GetWindowsDirectoryA
                | Self::GetSystemDirectoryA
                | Self::GetTempPathA
                | Self::GetDriveTypeA
                | Self::GetVolumeInformationA
                | Self::GetDiskFreeSpaceA
                | Self::SetCurrentDirectoryA
                | Self::GetFileAttributesA
                | Self::SetFileAttributesA
                | Self::FindFirstFileA
                | Self::FindClose
                | Self::QueryPerformanceFrequency
                | Self::QueryPerformanceCounter
                | Self::GetLocalTime
                | Self::GetSystemTime
                | Self::SystemTimeToFileTime
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
                | Self::CrtVsnprintf
                | Self::CrtMemmove
                | Self::CrtIsDigit
                | Self::CrtToUpper
                | Self::CrtAtol
                | Self::CrtSscanf
                | Self::CrtRand
                | Self::CrtSrand
                | Self::CrtStrncpy
                | Self::CrtStrpbrk
                | Self::CrtStrlwr
                | Self::CrtStrupr
                | Self::CrtStrncmp
                | Self::CrtStricmp
                | Self::CrtStrrchr
                | Self::CrtStrstr
                | Self::CrtStrnicmp
                | Self::CrtFullPath
                | Self::LoadStringA
                | Self::WsprintfA
                | Self::D3D8Release
                | Self::D3D8GetAdapterIdentifier
                | Self::EnumDisplayDevicesA
                | Self::EnumDisplaySettingsA
                | Self::ChangeDisplaySettingsExA
                | Self::GetDeviceCaps
                | Self::ReleaseDC
                | Self::WglMakeCurrent
                | Self::GlDisable
                | Self::GlEnable
                | Self::GlLightfv
                | Self::GlFogfv
                | Self::GlFogf
                | Self::GlFogi
                | Self::GlDrawBuffer
                | Self::GlDepthFunc
                | Self::GlAlphaFunc
                | Self::GlBlendFunc
                | Self::GlEnableClientState
                | Self::GlTexEnvi
                | Self::GlBindTexture
                | Self::GlDisableClientState
                | Self::GlDepthMask
                | Self::GlColorMaterial
                | Self::GlTexGeni
                | Self::GlLightModelfv
                | Self::GlMaterialfv
                | Self::GlPolygonOffset
                | Self::GlGetIntegerv
                | Self::WglGetProcAddress
                | Self::GlGetString
                | Self::WglCreateContext
                | Self::WglDeleteContext
                | Self::GlDeleteTextures
                | Self::GlTexSubImage2D
                | Self::GlTexImage2D
                | Self::GlPixelStorei
                | Self::GlTexParameteri
                | Self::GlGenTextures
                | Self::GlNormal3fv
                | Self::GlNormalPointer
                | Self::GlVertexPointer
                | Self::GlColorPointer
                | Self::GlTexCoordPointer
                | Self::GlFinish
                | Self::GlDrawElements
                | Self::GlLoadMatrixf
                | Self::GlMatrixMode
                | Self::GlScissor
                | Self::GlDepthRange
                | Self::GlViewport
                | Self::GlClear
                | Self::GlClearColor
                | Self::GlReadPixels
                | Self::GlReadBuffer
                | Self::WglSwapLayerBuffers
                | Self::GlLightf
                | Self::SetTextColor
                | Self::SetBkColor
                | Self::SetPixelFormat
                | Self::TextOutW
                | Self::SetDeviceGammaRamp
                | Self::DescribePixelFormat
                | Self::ChoosePixelFormat
                | Self::SetTextAlign
                | Self::SelectObject
                | Self::GetDeviceGammaRamp
                | Self::CreateFontA
                | Self::GetStockObject
                | Self::DeleteObject
                | Self::LoadImageA
                | Self::LoadCursorA
                | Self::RegisterClassA
                | Self::RegisterClassExA
                | Self::GetDesktopWindow
                | Self::ClipCursor
                | Self::EndPaint
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
            "GlobalMemoryStatus" => ProviderOp::GlobalMemoryStatus,
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
            "FreeLibrary" => ProviderOp::FreeLibrary,
            "GetProcAddress" => ProviderOp::GetProcAddress,
            "InterlockedExchange" => ProviderOp::InterlockedExchange,
            "InterlockedIncrement" => ProviderOp::InterlockedIncrement,
            "InterlockedDecrement" => ProviderOp::InterlockedDecrement,
            "TlsAlloc" => ProviderOp::TlsAlloc,
            "TlsSetValue" => ProviderOp::TlsSetValue,
            "TlsGetValue" => ProviderOp::TlsGetValue,
            "GetCurrentProcess" => ProviderOp::GetCurrentProcess,
            "GetCurrentProcessId" => ProviderOp::GetCurrentProcessId,
            "GetProcessHeap" => ProviderOp::GetProcessHeap,
            "GetCurrentThread" => ProviderOp::GetCurrentThread,
            "GetCurrentThreadId" => ProviderOp::GetCurrentThreadId,
            "ReadProcessMemory" => ProviderOp::ReadProcessMemory,
            "WriteProcessMemory" => ProviderOp::WriteProcessMemory,
            "GetLastError" => ProviderOp::GetLastError,
            "FormatMessageA" => ProviderOp::FormatMessageA,
            "GetTickCount" => ProviderOp::GetTickCount,
            "Sleep" => ProviderOp::Sleep,
            "CreateThread" => ProviderOp::CreateThread,
            "SetThreadPriority" => ProviderOp::SetThreadPriority,
            "GetThreadPriority" => ProviderOp::GetThreadPriority,
            "CreateFileA" => ProviderOp::CreateFileA,
            "GetFileSize" => ProviderOp::GetFileSize,
            "SetFilePointer" => ProviderOp::SetFilePointer,
            "ReadFile" => ProviderOp::ReadFile,
            "WriteFile" => ProviderOp::WriteFile,
            "FlushFileBuffers" => ProviderOp::FlushFileBuffers,
            "CreateEventA" => ProviderOp::CreateEventA,
            "OpenEventA" => ProviderOp::OpenEventA,
            "SetEvent" => ProviderOp::SetEvent,
            "ResetEvent" => ProviderOp::ResetEvent,
            "CreateMutexA" => ProviderOp::CreateMutexA,
            "ReleaseMutex" => ProviderOp::ReleaseMutex,
            "CloseHandle" => ProviderOp::CloseHandle,
            "WaitForSingleObject" => ProviderOp::WaitForSingleObject,
            "WaitForMultipleObjects" => ProviderOp::WaitForMultipleObjects,
            "GetWindowsDirectoryA" => ProviderOp::GetWindowsDirectoryA,
            "GetSystemDirectoryA" => ProviderOp::GetSystemDirectoryA,
            "GetTempPathA" => ProviderOp::GetTempPathA,
            "GetDriveTypeA" => ProviderOp::GetDriveTypeA,
            "GetVolumeInformationA" => ProviderOp::GetVolumeInformationA,
            "GetDiskFreeSpaceA" => ProviderOp::GetDiskFreeSpaceA,
            "SetCurrentDirectoryA" => ProviderOp::SetCurrentDirectoryA,
            "GetFileAttributesA" => ProviderOp::GetFileAttributesA,
            "SetFileAttributesA" => ProviderOp::SetFileAttributesA,
            "FindFirstFileA" => ProviderOp::FindFirstFileA,
            "FindClose" => ProviderOp::FindClose,
            "QueryPerformanceFrequency" => ProviderOp::QueryPerformanceFrequency,
            "QueryPerformanceCounter" => ProviderOp::QueryPerformanceCounter,
            "GetLocalTime" => ProviderOp::GetLocalTime,
            "GetSystemTime" => ProviderOp::GetSystemTime,
            "SystemTimeToFileTime" => ProviderOp::SystemTimeToFileTime,
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
            "DisableThreadLibraryCalls" => ProviderOp::DisableThreadLibraryCalls,
            "SetUnhandledExceptionFilter" => ProviderOp::SetUnhandledExceptionFilter,
            "UnhandledExceptionFilter" => ProviderOp::UnhandledExceptionFilter,
            "RtlUnwind" => ProviderOp::RtlUnwind,
            "ExitProcess" => ProviderOp::ExitProcess,
            "VirtualAlloc" => ProviderOp::VirtualAlloc,
            "VirtualFree" => ProviderOp::VirtualFree,
            _ => ProviderOp::Unknown,
        };
    }
    if import.module.eq_ignore_ascii_case("USER32.dll") {
        return match symbol.as_str() {
            "MessageBoxA" => ProviderOp::MessageBoxA,
            "LoadStringA" => ProviderOp::LoadStringA,
            "wsprintfA" => ProviderOp::WsprintfA,
            "EnumDisplayDevicesA" => ProviderOp::EnumDisplayDevicesA,
            "EnumDisplaySettingsA" => ProviderOp::EnumDisplaySettingsA,
            "ChangeDisplaySettingsExA" => ProviderOp::ChangeDisplaySettingsExA,
            "SetWindowPos" => ProviderOp::SetWindowPos,
            "ShowWindow" => ProviderOp::ShowWindow,
            "UpdateWindow" => ProviderOp::UpdateWindow,
            "SetFocus" => ProviderOp::SetFocus,
            "SetForegroundWindow" => ProviderOp::SetForegroundWindow,
            "GetDesktopWindow" => ProviderOp::GetDesktopWindow,
            "GetWindowRect" => ProviderOp::GetWindowRect,
            "SetWindowTextA" => ProviderOp::SetWindowTextA,
            "ClipCursor" => ProviderOp::ClipCursor,
            "BeginPaint" => ProviderOp::BeginPaint,
            "FillRect" => ProviderOp::FillRect,
            "EndPaint" => ProviderOp::EndPaint,
            "GetDC" => ProviderOp::GetDC,
            "ReleaseDC" => ProviderOp::ReleaseDC,
            "LoadImageA" => ProviderOp::LoadImageA,
            "LoadCursorA" => ProviderOp::LoadCursorA,
            "RegisterClassA" => ProviderOp::RegisterClassA,
            "RegisterClassExA" => ProviderOp::RegisterClassExA,
            "CreateWindowExA" => ProviderOp::CreateWindowExA,
            _ => ProviderOp::Unknown,
        };
    }
    if import.module.eq_ignore_ascii_case("IMM32.dll") {
        return match symbol.as_str() {
            "ImmAssociateContext" => ProviderOp::ImmAssociateContext,
            _ => ProviderOp::Unknown,
        };
    }
    if import.module.eq_ignore_ascii_case("GDI32.dll") {
        return match symbol.as_str() {
            "GetDeviceCaps" => ProviderOp::GetDeviceCaps,
            "SetTextColor" => ProviderOp::SetTextColor,
            "SetBkColor" => ProviderOp::SetBkColor,
            "SetPixelFormat" => ProviderOp::SetPixelFormat,
            "TextOutW" => ProviderOp::TextOutW,
            "SetDeviceGammaRamp" => ProviderOp::SetDeviceGammaRamp,
            "DescribePixelFormat" => ProviderOp::DescribePixelFormat,
            "ChoosePixelFormat" => ProviderOp::ChoosePixelFormat,
            "SetTextAlign" => ProviderOp::SetTextAlign,
            "SelectObject" => ProviderOp::SelectObject,
            "GetDeviceGammaRamp" => ProviderOp::GetDeviceGammaRamp,
            "CreateFontA" => ProviderOp::CreateFontA,
            "GetStockObject" => ProviderOp::GetStockObject,
            "DeleteObject" => ProviderOp::DeleteObject,
            _ => ProviderOp::Unknown,
        };
    }
    if import.module.eq_ignore_ascii_case("OPENGL32.dll") {
        return match symbol.as_str() {
            "wglMakeCurrent" => ProviderOp::WglMakeCurrent,
            "glDisable" => ProviderOp::GlDisable,
            "glEnable" => ProviderOp::GlEnable,
            "glLightfv" => ProviderOp::GlLightfv,
            "glFogfv" => ProviderOp::GlFogfv,
            "glFogf" => ProviderOp::GlFogf,
            "glFogi" => ProviderOp::GlFogi,
            "glDrawBuffer" => ProviderOp::GlDrawBuffer,
            "glDepthFunc" => ProviderOp::GlDepthFunc,
            "glAlphaFunc" => ProviderOp::GlAlphaFunc,
            "glBlendFunc" => ProviderOp::GlBlendFunc,
            "glEnableClientState" => ProviderOp::GlEnableClientState,
            "glTexEnvi" => ProviderOp::GlTexEnvi,
            "glBindTexture" => ProviderOp::GlBindTexture,
            "glDisableClientState" => ProviderOp::GlDisableClientState,
            "glDepthMask" => ProviderOp::GlDepthMask,
            "glColorMaterial" => ProviderOp::GlColorMaterial,
            "glTexGeni" => ProviderOp::GlTexGeni,
            "glLightModelfv" => ProviderOp::GlLightModelfv,
            "glMaterialfv" => ProviderOp::GlMaterialfv,
            "glPolygonOffset" => ProviderOp::GlPolygonOffset,
            "glGetIntegerv" => ProviderOp::GlGetIntegerv,
            "wglGetProcAddress" => ProviderOp::WglGetProcAddress,
            "glGetString" => ProviderOp::GlGetString,
            "wglCreateContext" => ProviderOp::WglCreateContext,
            "wglDeleteContext" => ProviderOp::WglDeleteContext,
            "glDeleteTextures" => ProviderOp::GlDeleteTextures,
            "glTexSubImage2D" => ProviderOp::GlTexSubImage2D,
            "glTexImage2D" => ProviderOp::GlTexImage2D,
            "glPixelStorei" => ProviderOp::GlPixelStorei,
            "glTexParameteri" => ProviderOp::GlTexParameteri,
            "glGenTextures" => ProviderOp::GlGenTextures,
            "glNormal3fv" => ProviderOp::GlNormal3fv,
            "glNormalPointer" => ProviderOp::GlNormalPointer,
            "glVertexPointer" => ProviderOp::GlVertexPointer,
            "glColorPointer" => ProviderOp::GlColorPointer,
            "glTexCoordPointer" => ProviderOp::GlTexCoordPointer,
            "glFinish" => ProviderOp::GlFinish,
            "glDrawElements" => ProviderOp::GlDrawElements,
            "glLoadMatrixf" => ProviderOp::GlLoadMatrixf,
            "glMatrixMode" => ProviderOp::GlMatrixMode,
            "glScissor" => ProviderOp::GlScissor,
            "glDepthRange" => ProviderOp::GlDepthRange,
            "glViewport" => ProviderOp::GlViewport,
            "glClear" => ProviderOp::GlClear,
            "glClearColor" => ProviderOp::GlClearColor,
            "glReadPixels" => ProviderOp::GlReadPixels,
            "glReadBuffer" => ProviderOp::GlReadBuffer,
            "wglSwapLayerBuffers" => ProviderOp::WglSwapLayerBuffers,
            "glLightf" => ProviderOp::GlLightf,
            _ => ProviderOp::Unknown,
        };
    }
    if import.module.eq_ignore_ascii_case("ADVAPI32.dll") {
        return match symbol.as_str() {
            "RegOpenKeyExA" => ProviderOp::RegOpenKeyExA,
            "RegQueryValueExA" => ProviderOp::RegQueryValueExA,
            "RegCloseKey" => ProviderOp::RegCloseKey,
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
            "_control87" => ProviderOp::CrtControl87,
            "_clearfp" => ProviderOp::CrtClearFp,
            "__getmainargs" => ProviderOp::CrtGetMainArgs,
            "_onexit" => ProviderOp::CrtOnExit,
            "_vsnprintf" => ProviderOp::CrtVsnprintf,
            "malloc" => ProviderOp::CrtMalloc,
            "memmove" => ProviderOp::CrtMemmove,
            "isdigit" => ProviderOp::CrtIsDigit,
            "toupper" => ProviderOp::CrtToUpper,
            "atol" => ProviderOp::CrtAtol,
            "sscanf" => ProviderOp::CrtSscanf,
            "_ftol" => ProviderOp::CrtFtol,
            "rand" => ProviderOp::CrtRand,
            "srand" => ProviderOp::CrtSrand,
            "strncpy" => ProviderOp::CrtStrncpy,
            "strpbrk" => ProviderOp::CrtStrpbrk,
            "_strlwr" => ProviderOp::CrtStrlwr,
            "_strupr" => ProviderOp::CrtStrupr,
            "strncmp" => ProviderOp::CrtStrncmp,
            "_stricmp" => ProviderOp::CrtStricmp,
            "strrchr" => ProviderOp::CrtStrrchr,
            "strstr" => ProviderOp::CrtStrstr,
            "_strnicmp" => ProviderOp::CrtStrnicmp,
            "_fullpath" => ProviderOp::CrtFullPath,
            "_beginthreadex" => ProviderOp::CrtBeginThreadEx,
            _ => ProviderOp::Unknown,
        };
    }
    if import.module.eq_ignore_ascii_case("d3d8.dll") && symbol == "Direct3DCreate8" {
        return ProviderOp::Direct3DCreate8;
    }
    if import.module.eq_ignore_ascii_case("d3d8.dll") && symbol == "IDirect3D8::Release" {
        return ProviderOp::D3D8Release;
    }
    if import.module.eq_ignore_ascii_case("d3d8.dll")
        && symbol == "IDirect3D8::GetAdapterIdentifier"
    {
        return ProviderOp::D3D8GetAdapterIdentifier;
    }
    ProviderOp::Unknown
}

pub fn provider_thunk_kind(import: &ProviderImport) -> thunk32::Kind {
    if provider_op(import) == ProviderOp::CrtMemmove && !cfg!(feature = "host-memmove") {
        return thunk32::Kind::Memmove;
    }
    match provider_op(import).stack_cleanup_bytes() {
        0 => thunk32::Kind::Return,
        bytes => thunk32::Kind::Stdcall(bytes),
    }
}

/// ABI contracts for exports which exist in an external provider even though
/// their behavior has not yet been modeled.  `GetProcAddress` must expose
/// these independently of [`ProviderOp`], so guest control flow reaches the
/// real API frontier rather than a fabricated missing-export branch.
pub fn external_export_thunk_kind(import: &ProviderImport) -> Option<thunk32::Kind> {
    match (&import.module[..], &import.symbol) {
        (module, ProviderSymbol::Name(symbol))
            if module.eq_ignore_ascii_case("d3d8.dll") && symbol == "Direct3DCreate8" =>
        {
            Some(thunk32::Kind::Stdcall(4))
        }
        _ if provider_op(import).is_modeled() => Some(provider_thunk_kind(import)),
        _ => None,
    }
}

#[cfg(test)]
mod beginthreadex_tests {
    use super::*;

    #[test]
    fn user32_fill_rect_has_stdcall_abi() {
        let import = ProviderImport {
            module: "USER32.dll".into(),
            symbol: ProviderSymbol::Name("FillRect".into()),
            iat_rva: 0,
        };
        assert_eq!(provider_op(&import), ProviderOp::FillRect);
        assert_eq!(provider_op(&import).stack_cleanup_bytes(), 12);
        assert_eq!(provider_thunk_kind(&import), thunk32::Kind::Stdcall(12));
    }

    #[test]
    fn static_gdi_imports_have_typed_frontier_dispatch_and_abi() {
        let entries = [
            ("SetTextColor", ProviderOp::SetTextColor, 8),
            ("SetBkColor", ProviderOp::SetBkColor, 8),
            ("GetDeviceCaps", ProviderOp::GetDeviceCaps, 8),
            ("SetPixelFormat", ProviderOp::SetPixelFormat, 12),
            ("TextOutW", ProviderOp::TextOutW, 20),
            ("SetDeviceGammaRamp", ProviderOp::SetDeviceGammaRamp, 8),
            ("DescribePixelFormat", ProviderOp::DescribePixelFormat, 16),
            ("ChoosePixelFormat", ProviderOp::ChoosePixelFormat, 8),
            ("SetTextAlign", ProviderOp::SetTextAlign, 8),
            ("SelectObject", ProviderOp::SelectObject, 8),
            ("GetDeviceGammaRamp", ProviderOp::GetDeviceGammaRamp, 8),
            ("CreateFontA", ProviderOp::CreateFontA, 56),
            ("GetStockObject", ProviderOp::GetStockObject, 4),
            ("DeleteObject", ProviderOp::DeleteObject, 4),
        ];
        for (symbol, expected, cleanup) in entries {
            let import = ProviderImport {
                module: "GDI32.dll".into(),
                symbol: ProviderSymbol::Name(symbol.into()),
                iat_rva: 0,
            };
            let operation = provider_op(&import);
            assert_eq!(operation, expected, "{symbol}");
            assert!(operation.is_generic_process_local(), "{symbol}");
            assert_eq!(operation.stack_cleanup_bytes(), cleanup, "{symbol}");
            assert_eq!(
                provider_thunk_kind(&import),
                thunk32::Kind::Stdcall(cleanup)
            );
        }
    }

    #[test]
    fn static_gl_imports_have_typed_frontier_dispatch_and_abi() {
        let entries = [
            ("wglMakeCurrent", 8),
            ("glDisable", 4),
            ("glEnable", 4),
            ("glLightfv", 12),
            ("glFogfv", 8),
            ("glFogf", 8),
            ("glFogi", 8),
            ("glDrawBuffer", 4),
            ("glDepthFunc", 4),
            ("glAlphaFunc", 8),
            ("glBlendFunc", 8),
            ("glEnableClientState", 4),
            ("glTexEnvi", 12),
            ("glBindTexture", 8),
            ("glDisableClientState", 4),
            ("glDepthMask", 4),
            ("glColorMaterial", 8),
            ("glTexGeni", 12),
            ("glLightModelfv", 8),
            ("glMaterialfv", 12),
            ("glPolygonOffset", 8),
            ("glGetIntegerv", 8),
            ("wglGetProcAddress", 4),
            ("glGetString", 4),
            ("wglCreateContext", 4),
            ("wglDeleteContext", 4),
            ("glDeleteTextures", 8),
            ("glTexSubImage2D", 36),
            ("glTexImage2D", 36),
            ("glPixelStorei", 8),
            ("glTexParameteri", 12),
            ("glGenTextures", 8),
            ("glNormal3fv", 4),
            ("glNormalPointer", 12),
            ("glVertexPointer", 16),
            ("glColorPointer", 16),
            ("glTexCoordPointer", 16),
            ("glFinish", 0),
            ("glDrawElements", 16),
            ("glLoadMatrixf", 4),
            ("glMatrixMode", 4),
            ("glScissor", 16),
            ("glDepthRange", 16),
            ("glViewport", 16),
            ("glClear", 4),
            ("glClearColor", 16),
            ("glReadPixels", 28),
            ("glReadBuffer", 4),
            ("wglSwapLayerBuffers", 8),
            ("glLightf", 12),
        ];
        for (symbol, cleanup) in entries {
            let import = ProviderImport {
                module: "OPENGL32.dll".into(),
                symbol: ProviderSymbol::Name(symbol.into()),
                iat_rva: 0,
            };
            let operation = provider_op(&import);
            assert_ne!(operation, ProviderOp::Unknown, "{symbol}");
            assert!(operation.is_generic_process_local(), "{symbol}");
            assert_eq!(operation.stack_cleanup_bytes(), cleanup, "{symbol}");
            assert_eq!(
                provider_thunk_kind(&import),
                thunk32::Kind::Stdcall(cleanup),
                "{symbol}",
            );
        }
    }

    #[test]
    fn d3d8_create8_is_an_advertised_stdcall_export() {
        let import = ProviderImport {
            module: "d3d8.dll".into(),
            symbol: ProviderSymbol::Name("Direct3DCreate8".into()),
            iat_rva: 0,
        };

        assert_eq!(provider_op(&import), ProviderOp::Direct3DCreate8);
        assert!(provider_op(&import).is_modeled());
        assert_eq!(
            external_export_thunk_kind(&import),
            Some(thunk32::Kind::Stdcall(4))
        );

        let mut bytes = [0u8; thunk32::THUNK_BYTES];
        thunk32::write(
            123,
            external_export_thunk_kind(&import).unwrap(),
            &mut bytes,
        )
        .unwrap();
        assert_eq!(&bytes[8..11], &[0xc2, 0x04, 0x00]);
    }

    #[test]
    fn d3d8_get_adapter_identifier_is_stdcall_and_process_local() {
        let import = ProviderImport {
            module: "d3d8.dll".into(),
            symbol: ProviderSymbol::Name("IDirect3D8::GetAdapterIdentifier".into()),
            iat_rva: 0,
        };

        let operation = provider_op(&import);
        assert_eq!(operation, ProviderOp::D3D8GetAdapterIdentifier);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 16);
        assert_eq!(provider_thunk_kind(&import), thunk32::Kind::Stdcall(16));
    }

    #[test]
    fn enum_display_devices_a_is_stdcall_and_process_local() {
        let import = ProviderImport {
            module: "USER32.dll".into(),
            symbol: ProviderSymbol::Name("EnumDisplayDevicesA".into()),
            iat_rva: 0,
        };

        let operation = provider_op(&import);
        assert_eq!(operation, ProviderOp::EnumDisplayDevicesA);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 16);
        assert_eq!(provider_thunk_kind(&import), thunk32::Kind::Stdcall(16));
    }

    #[test]
    fn enum_display_settings_a_is_stdcall_and_process_local() {
        let import = ProviderImport {
            module: "USER32.dll".into(),
            symbol: ProviderSymbol::Name("EnumDisplaySettingsA".into()),
            iat_rva: 0,
        };

        let operation = provider_op(&import);
        assert_eq!(operation, ProviderOp::EnumDisplaySettingsA);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 12);
        assert_eq!(provider_thunk_kind(&import), thunk32::Kind::Stdcall(12));
    }

    #[test]
    fn d3d8_release_is_stdcall_and_process_local() {
        let import = ProviderImport {
            module: "d3d8.dll".into(),
            symbol: ProviderSymbol::Name("IDirect3D8::Release".into()),
            iat_rva: 0,
        };

        let operation = provider_op(&import);
        assert_eq!(operation, ProviderOp::D3D8Release);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 4);
        assert_eq!(provider_thunk_kind(&import), thunk32::Kind::Stdcall(4));

        let mut bytes = [0u8; thunk32::THUNK_BYTES];
        thunk32::write(123, provider_thunk_kind(&import), &mut bytes).unwrap();
        assert_eq!(&bytes[8..11], &[0xc2, 0x04, 0x00]);
    }

    #[test]
    fn crt_clearfp_is_cdecl_and_context_owned() {
        let import = ProviderImport {
            module: "MSVCRT.dll".into(),
            symbol: ProviderSymbol::Name("_clearfp".into()),
            iat_rva: 0,
        };

        let operation = provider_op(&import);
        assert_eq!(operation, ProviderOp::CrtClearFp);
        assert!(operation.is_modeled());
        assert!(!operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 0);
        assert_eq!(provider_thunk_kind(&import), thunk32::Kind::Return);
    }

    #[test]
    fn crt_control87_is_cdecl_and_context_owned() {
        let import = ProviderImport {
            module: "MSVCRT.dll".into(),
            symbol: ProviderSymbol::Name("_control87".into()),
            iat_rva: 0,
        };

        let operation = provider_op(&import);
        assert_eq!(operation, ProviderOp::CrtControl87);
        assert!(operation.is_modeled());
        assert!(!operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 0);
        assert_eq!(provider_thunk_kind(&import), thunk32::Kind::Return);
    }

    #[test]
    fn beginthreadex_is_cdecl() {
        let import = ProviderImport {
            module: "MSVCRT.dll".into(),
            symbol: ProviderSymbol::Name("_beginthreadex".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&import);
        assert_eq!(operation, ProviderOp::CrtBeginThreadEx);
        assert!(operation.is_modeled());
        assert!(!operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 0);
        assert_eq!(provider_thunk_kind(&import), thunk32::Kind::Return);
        let mut bytes = [0u8; thunk32::THUNK_BYTES];
        thunk32::write(42, provider_thunk_kind(&import), &mut bytes).unwrap();
        assert_eq!(bytes[8], 0xc3);
    }

    #[test]
    fn sleep_is_stdcall_and_has_a_single_scheduler_argument() {
        let import = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("Sleep".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&import);
        assert_eq!(operation, ProviderOp::Sleep);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 4);
        assert_eq!(provider_thunk_kind(&import), thunk32::Kind::Stdcall(4));
        let mut bytes = [0u8; thunk32::THUNK_BYTES];
        thunk32::write(47, provider_thunk_kind(&import), &mut bytes).unwrap();
        assert_eq!(&bytes[8..11], &[0xc2, 0x04, 0x00]);
    }

    #[test]
    fn get_drive_type_a_is_stdcall_and_process_local() {
        let import = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetDriveTypeA".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&import);
        assert_eq!(operation, ProviderOp::GetDriveTypeA);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 4);
        assert_eq!(provider_thunk_kind(&import), thunk32::Kind::Stdcall(4));
    }

    #[test]
    fn get_volume_information_a_is_stdcall_and_process_local() {
        let import = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetVolumeInformationA".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&import);
        assert_eq!(operation, ProviderOp::GetVolumeInformationA);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 32);
        assert_eq!(provider_thunk_kind(&import), thunk32::Kind::Stdcall(32));
    }

    #[test]
    fn get_disk_free_space_a_is_stdcall_and_process_local() {
        let import = ProviderImport {
            module: "KERNEL32.dll".into(),
            symbol: ProviderSymbol::Name("GetDiskFreeSpaceA".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&import);
        assert_eq!(operation, ProviderOp::GetDiskFreeSpaceA);
        assert!(operation.is_modeled());
        assert!(operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 20);
        assert_eq!(provider_thunk_kind(&import), thunk32::Kind::Stdcall(20));
    }

    #[test]
    fn reg_query_value_ex_a_has_six_stdcall_arguments() {
        let import = ProviderImport {
            module: "ADVAPI32.dll".into(),
            symbol: ProviderSymbol::Name("RegQueryValueExA".into()),
            iat_rva: 0,
        };
        let operation = provider_op(&import);
        assert_eq!(operation, ProviderOp::RegQueryValueExA);
        assert!(operation.is_modeled());
        assert!(!operation.is_generic_process_local());
        assert_eq!(operation.stack_cleanup_bytes(), 24);
        assert_eq!(provider_thunk_kind(&import), thunk32::Kind::Stdcall(24));
        let mut bytes = [0u8; thunk32::THUNK_BYTES];
        thunk32::write(542, provider_thunk_kind(&import), &mut bytes).unwrap();
        assert_eq!(&bytes[8..11], &[0xc2, 0x18, 0x00]);
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
            external_export_thunk_kind(import).unwrap_or_else(|| provider_thunk_kind(import)),
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
