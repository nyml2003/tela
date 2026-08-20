//! Isolated macOS FFI boundary: IOKit power sources and SCNetworkReachability.
//!
//! All platform calls live here; the rest of the crate stays unsafe-free. Frameworks are linked
//! via `#[link(kind = "framework")]`.

#![allow(unsafe_code)]

use core::ffi::{c_char, c_int, c_void};

// ---------------------------------------------------------------------------
// CoreFoundation primitive types (as opaque pointers; we only pass them through).
// ---------------------------------------------------------------------------

type CFTypeRef = *const c_void;
type CFArrayRef = *const c_void;
type CFDictionaryRef = *const c_void;
type CFStringRef = *const c_void;
type CFNumberRef = *const c_void;
type CFBooleanRef = *const c_void;

/// kCFNumberSInt32Type：本模块用 u32 读取容量值。
const KCF_NUMBER_SINT32: i64 = 3;

// ---------------------------------------------------------------------------
// IOKit power sources.
// ---------------------------------------------------------------------------

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOPSCopyPowerSourcesInfo() -> CFTypeRef;
    fn IOPSCopyPowerSourcesList(blob: CFTypeRef) -> CFArrayRef;
    fn IOPSGetPowerSourceDescription(blob: CFTypeRef, ps: CFTypeRef) -> CFDictionaryRef;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFArrayGetCount(the_array: CFArrayRef) -> c_int;
    fn CFArrayGetValueAtIndex(the_array: CFArrayRef, index: c_int) -> CFTypeRef;
    fn CFDictionaryGetValue(the_dict: CFDictionaryRef, key: CFTypeRef) -> CFTypeRef;
    fn CFNumberGetValue(number: CFNumberRef, the_type: i64, value_ptr: *mut c_void) -> u8;
    fn CFBooleanGetValue(boolean: CFBooleanRef) -> u8;
    fn CFStringCreateWithCString(
        allocator: CFTypeRef,
        c_string: *const c_char,
        encoding: u32,
    ) -> CFStringRef;
    fn CFStringGetCString(
        string: CFStringRef,
        buffer: *mut c_char,
        buffer_size: c_int,
        encoding: u32,
    ) -> u8;
    fn CFRelease(cf: CFTypeRef);
}

const KCF_STRING_ENCODING_UTF8: u32 = 0x08000100;

/// Constant CFString keys (kIOPSCurrentCapacityKey etc.) are ordinary exported symbols.
extern "C" {
    // IOKit power source keys (declared as C pointers in the framework headers).
    static kIOPSCurrentCapacityKey: *const c_void;
    static kIOPSMaxCapacityKey: *const c_void;
    static kIOPSIsChargingKey: *const c_void;
    static kIOPSPowerSourceStateKey: *const c_void;
}

/// Reads the first power source's current/max capacity, returning level in 0.0..=1.0.
///
/// Returns `None` when there are no power sources (e.g. a desktop Mac on mains only).
pub fn battery_level() -> Option<f32> {
    let blob = unsafe { IOPSCopyPowerSourcesInfo() };
    if blob.is_null() {
        return None;
    }
    let list = unsafe { IOPSCopyPowerSourcesList(blob) };
    if list.is_null() {
        unsafe { CFRelease(blob) };
        return None;
    }
    let count = unsafe { CFArrayGetCount(list) };
    let mut level = None;
    if count > 0 {
        let source = unsafe { CFArrayGetValueAtIndex(list, 0) };
        let description = unsafe { IOPSGetPowerSourceDescription(blob, source) };
        if !description.is_null() {
            let current = unsafe { CFDictionaryGetValue(description, kIOPSCurrentCapacityKey) };
            let max = unsafe { CFDictionaryGetValue(description, kIOPSMaxCapacityKey) };
            let (Some(current), Some(max)) = (capacity(current), capacity(max)) else {
                level = None;
                unsafe { CFRelease(list) };
                unsafe { CFRelease(blob) };
                return level;
            };
            if max > 0 {
                level = Some((current as f32 / max as f32).clamp(0.0, 1.0));
            }
        }
    }
    unsafe { CFRelease(list) };
    unsafe { CFRelease(blob) };
    level
}

/// Reads the first power source's charging state.
pub fn battery_charging() -> Option<bool> {
    let blob = unsafe { IOPSCopyPowerSourcesInfo() };
    if blob.is_null() {
        return None;
    }
    let list = unsafe { IOPSCopyPowerSourcesList(blob) };
    if list.is_null() {
        unsafe { CFRelease(blob) };
        return None;
    }
    let count = unsafe { CFArrayGetCount(list) };
    let mut charging = None;
    if count > 0 {
        let source = unsafe { CFArrayGetValueAtIndex(list, 0) };
        let description = unsafe { IOPSGetPowerSourceDescription(blob, source) };
        if !description.is_null() {
            let value = unsafe { CFDictionaryGetValue(description, kIOPSIsChargingKey) };
            if !value.is_null() {
                charging = Some(unsafe { CFBooleanGetValue(value as CFBooleanRef) } != 0);
            }
        }
    }
    unsafe { CFRelease(list) };
    unsafe { CFRelease(blob) };
    charging
}

fn capacity(value: CFTypeRef) -> Option<u32> {
    if value.is_null() {
        return None;
    }
    let mut number = 0u32;
    let ok = unsafe {
        CFNumberGetValue(
            value as CFNumberRef,
            KCF_NUMBER_SINT32,
            &mut number as *mut u32 as *mut c_void,
        )
    };
    if ok == 0 { None } else { Some(number) }
}

// ---------------------------------------------------------------------------
// SCNetworkReachability.
// ---------------------------------------------------------------------------

type SCNetworkReachabilityRef = *const c_void;

const K_SC_NETWORK_REACHABILITY_FLAGS_REACHABLE: u32 = 1 << 1;

#[link(name = "SystemConfiguration", kind = "framework")]
extern "C" {
    fn SCNetworkReachabilityCreateWithName(
        allocator: CFTypeRef,
        nodename: *const c_char,
    ) -> SCNetworkReachabilityRef;
    fn SCNetworkReachabilityGetFlags(target: SCNetworkReachabilityRef, flags: *mut u32) -> u8;
}

/// Synchronous reachability check against a well-known host name.
pub fn network_reachable() -> bool {
    let name = c"example.com";
    let reachability =
        unsafe { SCNetworkReachabilityCreateWithName(std::ptr::null(), name.as_ptr()) };
    if reachability.is_null() {
        return false;
    }
    let mut flags = 0u32;
    let ok = unsafe { SCNetworkReachabilityGetFlags(reachability, &mut flags) };
    unsafe { CFRelease(reachability) };
    ok != 0 && flags & K_SC_NETWORK_REACHABILITY_FLAGS_REACHABLE != 0
}
