//! Windows 控制端到目标 DLL 的最小注入与共享状态桥。

#![cfg(target_os = "windows")]
#![allow(unsafe_code)]

use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use tela_speed_gear_protocol::{HEARTBEAT_TIMEOUT_MS, NORMAL_RATE_MILLI, SHARED_SIZE, SharedState};
use windows::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::System::Diagnostics::Debug::WriteProcessMemory;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Memory::{
    CreateFileMappingW, FILE_MAP_ALL_ACCESS, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE,
    MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile, PAGE_READWRITE, UnmapViewOfFile, VirtualAllocEx,
    VirtualFreeEx,
};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::System::Threading::{
    CreateRemoteThread, OpenProcess, PROCESS_CREATE_THREAD, PROCESS_QUERY_INFORMATION,
    PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
};
use windows::core::{PCSTR, PCWSTR};

pub struct HookConnection {
    pub name: String,
    mapping: HANDLE,
    view: *mut SharedState,
    started_at: Instant,
}

impl HookConnection {
    pub fn connect(pid: u32, creation_time: u64) -> Result<Self, String> {
        let name = format!("Local\\TelaSpeedGear-{}", pid);
        let wide = name.encode_utf16().chain([0]).collect::<Vec<_>>();
        let mapping = unsafe {
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                None,
                PAGE_READWRITE,
                0,
                SHARED_SIZE as u32,
                PCWSTR(wide.as_ptr()),
            )
        }
        .map_err(|error| format!("CreateFileMappingW: {error}"))?;
        let view = unsafe { MapViewOfFile(mapping, FILE_MAP_ALL_ACCESS, 0, 0, SHARED_SIZE) };
        if view.Value.is_null() {
            unsafe { CloseHandle(mapping) }.ok();
            return Err("MapViewOfFile failed".to_owned());
        }
        let state = view.Value.cast::<SharedState>();
        unsafe {
            state.write(SharedState::new());
            (*state)
                .heartbeat_ms
                .store(GetTickCount64(), std::sync::atomic::Ordering::Release);
        }
        let Some(dll) = hook_dll_path() else {
            unsafe {
                UnmapViewOfFile(view).ok();
                CloseHandle(mapping).ok();
            }
            return Err("找不到 tela-speed-gear-hook.dll".to_owned());
        };
        if let Err(error) = inject(pid, &dll) {
            unsafe {
                UnmapViewOfFile(view).ok();
                CloseHandle(mapping).ok();
            }
            return Err(error);
        }
        let _ = creation_time;
        Ok(Self {
            name,
            mapping,
            view: state,
            started_at: Instant::now(),
        })
    }

    pub fn initialized(&self) -> bool {
        unsafe {
            (*self.view)
                .initialized
                .load(std::sync::atomic::Ordering::Acquire)
                != 0
        }
    }

    pub fn timed_out(&self) -> bool {
        self.started_at.elapsed() >= Duration::from_secs(3)
    }

    pub fn set_rate(&self, rate_milli: u64) {
        unsafe {
            (*self.view)
                .rate_milli
                .store(rate_milli, std::sync::atomic::Ordering::Release);
            (*self.view)
                .heartbeat_ms
                .store(GetTickCount64(), std::sync::atomic::Ordering::Release);
        }
    }

    pub fn heartbeat(&self) {
        unsafe {
            (*self.view)
                .heartbeat_ms
                .store(GetTickCount64(), std::sync::atomic::Ordering::Release)
        };
    }

    pub fn stop(&self) {
        self.set_rate(NORMAL_RATE_MILLI);
    }
}

impl Drop for HookConnection {
    fn drop(&mut self) {
        unsafe {
            let _ = UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                Value: self.view.cast(),
            });
            let _ = CloseHandle(self.mapping);
        }
    }
}

fn hook_dll_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("TELA_SPEED_GEAR_HOOK_DLL") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    let executable = std::env::current_exe().ok()?;
    let sibling = executable.parent()?.join("tela-speed-gear-hook.dll");
    sibling.is_file().then_some(sibling)
}

fn inject(pid: u32, dll: &Path) -> Result<(), String> {
    let rights = PROCESS_CREATE_THREAD
        | PROCESS_QUERY_INFORMATION
        | PROCESS_VM_OPERATION
        | PROCESS_VM_WRITE
        | PROCESS_VM_READ;
    let process = unsafe { OpenProcess(rights, false, pid) }
        .map_err(|error| format!("OpenProcess: {error}"))?;
    let wide = dll
        .as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain([0])
        .collect::<Vec<_>>();
    let bytes = wide.len() * size_of::<u16>();
    let remote = unsafe {
        VirtualAllocEx(
            process,
            None,
            bytes,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        )
    };
    if remote.is_null() {
        unsafe { CloseHandle(process) }.ok();
        return Err("VirtualAllocEx failed".to_owned());
    }
    let result = (|| {
        unsafe { WriteProcessMemory(process, remote, wide.as_ptr().cast(), bytes, None) }
            .map_err(|error| format!("WriteProcessMemory: {error}"))?;
        let kernel32_name = [
            'k' as u16, 'e' as u16, 'r' as u16, 'n' as u16, 'e' as u16, 'l' as u16, '3' as u16,
            '2' as u16, '.' as u16, 'd' as u16, 'l' as u16, 'l' as u16, 0,
        ];
        let kernel32 = unsafe { GetModuleHandleW(PCWSTR(kernel32_name.as_ptr())) }
            .map_err(|error| format!("GetModuleHandleW: {error}"))?;
        let load = unsafe { GetProcAddress(kernel32, PCSTR(c"LoadLibraryW".as_ptr().cast())) }
            .ok_or_else(|| "GetProcAddress(LoadLibraryW) failed".to_owned())?;
        let start = unsafe { std::mem::transmute(load) };
        let thread = unsafe { CreateRemoteThread(process, None, 0, start, Some(remote), 0, None) }
            .map_err(|error| format!("CreateRemoteThread: {error}"))?;
        unsafe { CloseHandle(thread) }.ok();
        Ok(())
    })();
    unsafe {
        let _ = VirtualFreeEx(process, remote, 0, MEM_RELEASE);
        let _ = CloseHandle(process);
    }
    result
}

pub fn now() -> u64 {
    unsafe { GetTickCount64() }
}

pub const fn timeout_ms() -> u64 {
    HEARTBEAT_TIMEOUT_MS
}
