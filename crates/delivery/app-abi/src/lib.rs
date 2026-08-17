//! Portable application ABI shared by Tela platform SDKs.
//!
//! The guest module owns application state, input routing, layout and frame creation. A native
//! SDK owns its platform event loop and renderer, and exchanges only explicit binary packets with
//! the guest. Rust values, pointers and trait objects never cross this boundary.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod error;
mod event;
mod frame;

pub use error::FrameCodecError;
pub use event::{
    AppEvent, AppPointerEvent, AppPointerKind, AppPointerPhase, AppStatus, CursorKind,
    decode_event, decode_status, encode_event, encode_status,
};
pub use frame::{WireFrame, decode_frame, encode_frame};

/// Exports the stable Tela guest ABI for one concrete application type.
///
/// The macro intentionally only removes byte-buffer and export boilerplate. Callers retain their
/// own application state, event mapping, and status projection, so this does not create a
/// runtime application interface or dynamic dispatch boundary.
///
/// ```ignore
/// tela_app_abi::export_guest! {
///     reset = crate::reset_app;
///     with_app = crate::with_app;
///     apply = apply_event;
///     publish = publish_app;
/// }
/// ```
///
/// `apply` receives `(&mut App, AppEvent) -> bool`. `publish` receives `&mut App` and returns
/// `Result<(&UiFrame, AppStatus), String>`. `with_app` is the application's synchronous access
/// helper and `reset` clears its concrete application state.
#[macro_export]
macro_rules! export_guest {
    {
        reset = $reset:path;
        with_app = $with_app:path;
        apply = $apply:path;
        publish = $publish:path;
    } => {
        ::std::thread_local! {
            static __TELA_INPUT_BYTES: ::std::cell::RefCell<::std::vec::Vec<u8>> = const {
                ::std::cell::RefCell::new(::std::vec::Vec::new())
            };
            static __TELA_FRAME_BYTES: ::std::cell::RefCell<::std::vec::Vec<u8>> = const {
                ::std::cell::RefCell::new(::std::vec::Vec::new())
            };
            static __TELA_STATUS_BYTES: ::std::cell::RefCell<::std::vec::Vec<u8>> = const {
                ::std::cell::RefCell::new(::std::vec::Vec::new())
            };
            static __TELA_ERROR_BYTES: ::std::cell::RefCell<::std::vec::Vec<u8>> = const {
                ::std::cell::RefCell::new(::std::vec::Vec::new())
            };
        }

        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn tela_app_abi_version() -> u32 {
            $crate::ABI_VERSION
        }

        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn tela_app_init() -> u32 {
            $reset();
            u32::from(__tela_publish())
        }

        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn tela_app_input_begin(bytes: u32) -> *mut u8 {
            __TELA_INPUT_BYTES.with(|input| {
                let mut input = input.borrow_mut();
                input.resize(bytes as usize, 0);
                input.as_mut_ptr()
            })
        }

        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn tela_app_dispatch(bytes: u32) -> u32 {
            let event = __TELA_INPUT_BYTES.with(|input| {
                let mut input = input.borrow_mut();
                if input.len() != bytes as usize {
                    input.clear();
                    return Err("input byte length changed before dispatch".to_owned());
                }
                $crate::decode_event(&input).map_err(|error| error.to_string())
            });
            let Ok(event) = event else {
                __tela_set_error(event.unwrap_err());
                return 0;
            };
            let changed = $with_app(|app| $apply(app, event));
            if !__tela_publish() {
                return 0;
            }
            u32::from(changed)
        }

        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn tela_app_frame_ptr() -> *const u8 {
            __TELA_FRAME_BYTES.with(|frame| frame.borrow().as_ptr())
        }

        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn tela_app_frame_len() -> u32 {
            __TELA_FRAME_BYTES.with(|frame| frame.borrow().len() as u32)
        }

        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn tela_app_status_ptr() -> *const u8 {
            __TELA_STATUS_BYTES.with(|status| status.borrow().as_ptr())
        }

        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn tela_app_status_len() -> u32 {
            __TELA_STATUS_BYTES.with(|status| status.borrow().len() as u32)
        }

        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn tela_app_error_ptr() -> *const u8 {
            __TELA_ERROR_BYTES.with(|error| error.borrow().as_ptr())
        }

        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn tela_app_error_len() -> u32 {
            __TELA_ERROR_BYTES.with(|error| error.borrow().len() as u32)
        }

        fn __tela_publish() -> bool {
            let published = $with_app(|app| {
                let (frame, status) = $publish(app)?;
                let frame = $crate::encode_frame(frame).map_err(|error| error.to_string())?;
                let status = $crate::encode_status(&status).map_err(|error| error.to_string())?;
                Ok::<_, ::std::string::String>((frame, status))
            });
            match published {
                Ok((frame, status)) => {
                    __TELA_FRAME_BYTES.with(|slot| *slot.borrow_mut() = frame);
                    __TELA_STATUS_BYTES.with(|slot| *slot.borrow_mut() = status);
                    __TELA_ERROR_BYTES.with(|slot| slot.borrow_mut().clear());
                    true
                }
                Err(error) => {
                    __tela_set_error(error);
                    false
                }
            }
        }

        fn __tela_set_error(error: ::std::string::String) {
            __TELA_ERROR_BYTES.with(|slot| *slot.borrow_mut() = error.into_bytes());
        }
    };
}

/// ABI version expected by the current development bundle runtime.
///
/// Version 3 replaces preclassified pointer variants with one complete raw pointer packet. Hosts
/// must reject a bundle whose declared version does not exactly match this value.
pub const ABI_VERSION: u32 = 3;
