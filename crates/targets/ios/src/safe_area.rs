//! UIKit safe-area access through the Xcode-owned Objective-C helper.

use std::ffi::c_void;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use tela_contract::Insets;
use winit::window::Window;

#[repr(C)]
struct NativeInsets {
    top: f32,
    right: f32,
    bottom: f32,
    left: f32,
}

unsafe extern "C" {
    fn tela_ios_safe_area_for_view(view: *mut c_void, out: *mut NativeInsets) -> bool;
}

/// Reads the current UIKit layout exclusions in the same logical-point space as Winit's viewport.
pub(super) fn for_window(window: &Window) -> Insets {
    let Ok(handle) = window.window_handle() else {
        return Insets::all(0.0);
    };
    let RawWindowHandle::UiKit(handle) = handle.as_raw() else {
        return Insets::all(0.0);
    };
    let mut native = NativeInsets {
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
        left: 0.0,
    };
    // SAFETY: Winit supplied the live UIView pointer for this window handle. The Objective-C
    // helper only reads `safeAreaInsets` synchronously on the UIKit main thread.
    let available = unsafe {
        tela_ios_safe_area_for_view(handle.ui_view.as_ptr(), std::ptr::addr_of_mut!(native))
    };
    if !available {
        return Insets::all(0.0);
    }
    Insets {
        top: native.top.max(0.0),
        right: native.right.max(0.0),
        bottom: native.bottom.max(0.0),
        left: native.left.max(0.0),
    }
}
