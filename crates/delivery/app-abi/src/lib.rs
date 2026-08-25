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
mod publication;

pub use error::FrameCodecError;
pub use event::{decode_event, decode_status, encode_event, encode_status};
pub use frame::{WireFrame, decode_frame, encode_frame};
pub use publication::{decode_publication, encode_publication};
pub use tela_app_session::{
    AppDispatchOutcome, AppEffect, AppEvent, AppFrameInput, AppFrameToken, AppPointerEvent,
    AppPointerKind, AppPointerPhase, AppPublication, AppStatus, ApplicationSession, CursorKind,
    SessionError,
};

/// Guest call completed successfully.
pub const OUTCOME_OK: u32 = 1 << 31;
/// Guest handled the delivered event.
pub const OUTCOME_HANDLED: u32 = 1;
/// Guest requests an explicit publication.
pub const OUTCOME_PUBLISH_REQUESTED: u32 = 1 << 1;

/// Converts ABI outcome bits into the logical dispatch result.
pub fn decode_outcome(bits: u32) -> Option<AppDispatchOutcome> {
    (bits & OUTCOME_OK != 0).then_some(AppDispatchOutcome {
        handled: bits & OUTCOME_HANDLED != 0,
        publish_requested: bits & OUTCOME_PUBLISH_REQUESTED != 0,
    })
}

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
            static __TELA_PUBLICATION_BYTES: ::std::cell::RefCell<::std::vec::Vec<u8>> = const {
                ::std::cell::RefCell::new(::std::vec::Vec::new())
            };
            static __TELA_ERROR_BYTES: ::std::cell::RefCell<::std::vec::Vec<u8>> = const {
                ::std::cell::RefCell::new(::std::vec::Vec::new())
            };
            static __TELA_PUBLISHED_TOKEN: ::std::cell::Cell<u64> = const {
                ::std::cell::Cell::new(0)
            };
            static __TELA_PRESENTED_TOKEN: ::std::cell::Cell<u64> = const {
                ::std::cell::Cell::new(0)
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
            __TELA_PUBLICATION_BYTES.with(|slot| slot.borrow_mut().clear());
            __TELA_ERROR_BYTES.with(|slot| slot.borrow_mut().clear());
            __TELA_PUBLISHED_TOKEN.with(|slot| slot.set(0));
            __TELA_PRESENTED_TOKEN.with(|slot| slot.set(0));
            $crate::OUTCOME_OK | $crate::OUTCOME_PUBLISH_REQUESTED
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
            if let $crate::AppEvent::FrameInput {
                source_frame_token,
                ..
            } = &event
                && __TELA_PRESENTED_TOKEN.with(|slot| slot.get()) != source_frame_token.get()
            {
                __TELA_ERROR_BYTES.with(|slot| slot.borrow_mut().clear());
                return $crate::OUTCOME_OK;
            }
            let changed = $with_app(|app| $apply(app, event));
            __TELA_ERROR_BYTES.with(|slot| slot.borrow_mut().clear());
            $crate::OUTCOME_OK
                | if changed {
                    $crate::OUTCOME_HANDLED | $crate::OUTCOME_PUBLISH_REQUESTED
                } else {
                    0
                }
        }

        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn tela_app_publish() -> u32 {
            if __tela_publish() {
                $crate::OUTCOME_OK
            } else {
                0
            }
        }

        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn tela_app_publication_ptr() -> *const u8 {
            __TELA_PUBLICATION_BYTES.with(|publication| publication.borrow().as_ptr())
        }

        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn tela_app_publication_len() -> u32 {
            __TELA_PUBLICATION_BYTES.with(|publication| publication.borrow().len() as u32)
        }

        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn tela_app_presented(token_low: u32, token_high: u32) -> u32 {
            let token = u64::from(token_low) | (u64::from(token_high) << 32);
            if token == 0 || __TELA_PUBLISHED_TOKEN.with(|slot| slot.get()) != token {
                __tela_set_error("presented token is not the latest publication".to_owned());
                return 0;
            }
            __TELA_PRESENTED_TOKEN.with(|slot| slot.set(token));
            __TELA_PUBLISHED_TOKEN.with(|slot| slot.set(0));
            __TELA_ERROR_BYTES.with(|slot| slot.borrow_mut().clear());
            $crate::OUTCOME_OK
        }

        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn tela_app_rejected(token_low: u32, token_high: u32) -> u32 {
            let token = u64::from(token_low) | (u64::from(token_high) << 32);
            if token == 0 || __TELA_PUBLISHED_TOKEN.with(|slot| slot.get()) != token {
                __tela_set_error("rejected token is not the latest publication".to_owned());
                return 0;
            }
            __TELA_PUBLISHED_TOKEN.with(|slot| slot.set(0));
            __TELA_ERROR_BYTES.with(|slot| slot.borrow_mut().clear());
            $crate::OUTCOME_OK
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
                let token = status
                    .frame_token
                    .ok_or_else(|| "publication status must contain a frame token".to_owned())?;
                let publication = $crate::AppPublication {
                    token,
                    frame: frame.clone(),
                    status,
                    effects: ::std::vec::Vec::new(),
                };
                let bytes = $crate::encode_publication(&publication)
                    .map_err(|error| error.to_string())?;
                Ok::<_, ::std::string::String>((token, bytes))
            });
            match published {
                Ok((token, bytes)) => {
                    __TELA_PUBLICATION_BYTES.with(|slot| *slot.borrow_mut() = bytes);
                    __TELA_PUBLISHED_TOKEN.with(|slot| slot.set(token.get()));
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
/// Version 6 separates event dispatch from atomic publication and adds presentation
/// acknowledgement. Hosts reject bundles whose declared version does not exactly match.
pub const ABI_VERSION: u32 = 6;
