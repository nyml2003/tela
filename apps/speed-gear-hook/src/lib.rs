//! 目标端 QPC 调速模块。
//!
//! 首版只改目标主模块导入表中的 `QueryPerformanceCounter`。这是有意收敛的作用范围：
//! 不改其它时钟、不卸载目标线程中的 DLL，并通过共享心跳在控制端消失后恢复正常速率。

#![cfg(target_os = "windows")]
#![allow(non_snake_case)]

use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicPtr, AtomicU64, Ordering};

use tela_speed_gear_protocol::{HEARTBEAT_TIMEOUT_MS, SharedState};
use windows::Win32::Foundation::HINSTANCE;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Memory::{FILE_MAP_ALL_ACCESS, MapViewOfFile, OpenFileMappingW};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::core::{BOOL, PCSTR, PCWSTR};

type QpcFn = unsafe extern "system" fn(*mut i64) -> BOOL;

static SHARED: AtomicPtr<SharedState> = AtomicPtr::new(null_mut());
static ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static LAST_RAW: AtomicU64 = AtomicU64::new(0);
static BASE_RAW: AtomicU64 = AtomicU64::new(0);
static BASE_VIRTUAL: AtomicU64 = AtomicU64::new(0);
static ACTIVE_RATE: AtomicU64 = AtomicU64::new(1_000);
static UPDATE_LOCK: AtomicU64 = AtomicU64::new(0);

#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllMain(_: HINSTANCE, reason: u32, _: *mut c_void) -> BOOL {
    if reason == DLL_PROCESS_ATTACH {
        let _ = unsafe { install_for_current_process() };
    }
    BOOL(1)
}

unsafe fn install_for_current_process() -> bool {
    let name = format!("Local\\TelaSpeedGear-{}", unsafe { GetCurrentProcessId() });
    let wide = name.encode_utf16().chain([0]).collect::<Vec<_>>();
    let mapping =
        match unsafe { OpenFileMappingW(FILE_MAP_ALL_ACCESS.0, false, PCWSTR(wide.as_ptr())) } {
            Ok(value) => value,
            Err(_) => return false,
        };
    let view = unsafe {
        MapViewOfFile(
            mapping,
            FILE_MAP_ALL_ACCESS,
            0,
            0,
            std::mem::size_of::<SharedState>(),
        )
    };
    if view.Value.is_null() {
        return false;
    }
    let shared = view.Value.cast::<SharedState>();
    if unsafe { (*shared).version.load(Ordering::Acquire) }
        != tela_speed_gear_protocol::PROTOCOL_VERSION
    {
        return false;
    }
    SHARED.store(shared, Ordering::Release);

    let kernel32_name = [
        'k' as u16, 'e' as u16, 'r' as u16, 'n' as u16, 'e' as u16, 'l' as u16, '3' as u16,
        '2' as u16, '.' as u16, 'd' as u16, 'l' as u16, 'l' as u16, 0,
    ];
    let kernel32 = match unsafe { GetModuleHandleW(PCWSTR(kernel32_name.as_ptr())) } {
        Ok(value) => value,
        Err(_) => return false,
    };
    let original =
        unsafe { GetProcAddress(kernel32, PCSTR(c"QueryPerformanceCounter".as_ptr().cast())) };
    let Some(original) = original else {
        return false;
    };
    ORIGINAL.store(original as *mut c_void, Ordering::Release);
    let main = match unsafe { GetModuleHandleW(PCWSTR(null())) } {
        Ok(value) => value,
        Err(_) => return false,
    };
    if !unsafe {
        patch_import_table(
            main.0.cast::<u8>(),
            original as *mut c_void,
            qpc_hook as *mut c_void,
        )
    } {
        return false;
    }
    unsafe { (*shared).initialized.store(1, Ordering::Release) };
    true
}

unsafe extern "system" fn qpc_hook(value: *mut i64) -> BOOL {
    let original = ORIGINAL.load(Ordering::Acquire);
    if original.is_null() {
        return BOOL(0);
    }
    let original: QpcFn = unsafe { std::mem::transmute(original) };
    let result = unsafe { original(value) };
    if result.0 == 0 || value.is_null() {
        return result;
    }
    let shared = SHARED.load(Ordering::Acquire);
    if shared.is_null() {
        return result;
    }
    let raw = unsafe { *value as u64 };
    let now = unsafe { GetTickCount64() };
    let mut rate = unsafe { (*shared).rate_milli.load(Ordering::Acquire) };
    let heartbeat = unsafe { (*shared).heartbeat_ms.load(Ordering::Acquire) };
    if heartbeat == 0 || now.saturating_sub(heartbeat) > HEARTBEAT_TIMEOUT_MS {
        rate = 1_000;
    }

    while UPDATE_LOCK
        .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        std::hint::spin_loop();
    }
    let previous_raw = LAST_RAW.load(Ordering::Relaxed);
    let previous_rate = ACTIVE_RATE.load(Ordering::Relaxed);
    if previous_raw == 0 {
        BASE_RAW.store(raw, Ordering::Relaxed);
        BASE_VIRTUAL.store(raw, Ordering::Relaxed);
    } else if previous_rate != rate {
        let current = scaled(
            raw,
            BASE_RAW.load(Ordering::Relaxed),
            BASE_VIRTUAL.load(Ordering::Relaxed),
            previous_rate,
        );
        BASE_RAW.store(raw, Ordering::Relaxed);
        BASE_VIRTUAL.store(current, Ordering::Relaxed);
        ACTIVE_RATE.store(rate, Ordering::Relaxed);
    }
    let virtual_value = scaled(
        raw,
        BASE_RAW.load(Ordering::Relaxed),
        BASE_VIRTUAL.load(Ordering::Relaxed),
        ACTIVE_RATE.load(Ordering::Relaxed),
    );
    LAST_RAW.store(raw, Ordering::Relaxed);
    UPDATE_LOCK.store(0, Ordering::Release);
    unsafe { *value = virtual_value as i64 };
    result
}

fn scaled(raw: u64, base_raw: u64, base_virtual: u64, rate: u64) -> u64 {
    base_virtual.saturating_add(raw.saturating_sub(base_raw).saturating_mul(rate) / 1_000)
}

unsafe fn patch_import_table(
    base: *mut u8,
    original: *mut c_void,
    replacement: *mut c_void,
) -> bool {
    if base.is_null() || unsafe { *(base as *const u16) } != 0x5a4d {
        return false;
    }
    let nt_offset = unsafe { *(base.add(0x3c) as *const i32) } as usize;
    let nt = unsafe { base.add(nt_offset) };
    if unsafe { *(nt as *const u32) } != 0x0000_4550 {
        return false;
    }
    let optional = unsafe { nt.add(4 + 20) };
    let magic = unsafe { *(optional as *const u16) };
    let data_dir = if magic == 0x20b {
        unsafe { optional.add(112) }
    } else {
        unsafe { optional.add(96) }
    };
    let import_rva = unsafe { *(data_dir.add(8) as *const u32) };
    if import_rva == 0 {
        return false;
    }
    let mut descriptor = unsafe { base.add(import_rva as usize) };
    loop {
        let original_thunk = unsafe { *(descriptor as *const u32) };
        let name_rva = unsafe { *(descriptor.add(12) as *const u32) };
        let first_thunk = unsafe { *(descriptor.add(16) as *const u32) };
        if original_thunk == 0 || first_thunk == 0 {
            break;
        }
        if name_rva != 0 {
            let mut thunk = unsafe { base.add(original_thunk as usize) };
            let mut iat = unsafe { base.add(first_thunk as usize) as *mut *mut c_void };
            loop {
                let value = unsafe { *(thunk as *const usize) };
                if value == 0 {
                    break;
                }
                if value & (1usize << (usize::BITS - 1)) == 0 {
                    let hint_name = unsafe { base.add(value as usize + 2) };
                    let mut length = 0;
                    while unsafe { *hint_name.add(length) } != 0 {
                        length += 1;
                    }
                    let name = unsafe { std::slice::from_raw_parts(hint_name, length) };
                    if name == b"QueryPerformanceCounter" && unsafe { *iat } == original {
                        let mut old = windows::Win32::System::Memory::PAGE_PROTECTION_FLAGS(0);
                        if unsafe {
                            windows::Win32::System::Memory::VirtualProtect(
                                iat.cast(),
                                std::mem::size_of::<*mut c_void>(),
                                windows::Win32::System::Memory::PAGE_READWRITE,
                                &mut old,
                            )
                        }
                        .is_err()
                        {
                            return false;
                        }
                        unsafe {
                            *iat = replacement;
                        }
                        let _ = unsafe {
                            windows::Win32::System::Memory::VirtualProtect(
                                iat.cast(),
                                std::mem::size_of::<*mut c_void>(),
                                old,
                                &mut old,
                            )
                        };
                        return true;
                    }
                }
                thunk = unsafe { thunk.add(std::mem::size_of::<usize>()) };
                iat = unsafe { iat.add(1) };
            }
        }
        descriptor = unsafe { descriptor.add(20) };
    }
    false
}
