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
mod transport;

use tela_contract::{DirtyFlags, FrameDamage, UiFrame};

pub use error::FrameCodecError;
pub use event::{decode_event, decode_status, encode_event, encode_status};
pub use frame::{WireFrame, decode_frame, encode_frame};
pub use publication::{decode_publication, encode_publication};
pub use tela_app_session::{
    AppDispatchOutcome, AppEffect, AppEvent, AppFrameInput, AppFrameToken, AppPointerEvent,
    AppPointerKind, AppPointerPhase, AppPublication, AppStatus, ApplicationSession, CursorKind,
    RetainedTreeSnapshot, SessionError,
};
pub use transport::{
    AppliedFrameTransport, FrameTransportPacket, FrameTransportReceiver, FrameTransportSender,
    TransportPublication, decode_transport_publication, encode_transport_publication,
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

/// 把 `apply` 回调的返回值胁迫为完整派发结果。
///
/// `bool` 保持旧语义（handled 与 publish_requested 同值）；返回
/// [`AppDispatchOutcome`] 的应用可以表达"未处理但投影脏"的发布请求，避免宿主循环
/// 停摆。
pub trait IntoDispatchOutcome {
    /// 转换为完整派发结果。
    fn into_dispatch_outcome(self) -> AppDispatchOutcome;
}

impl IntoDispatchOutcome for bool {
    fn into_dispatch_outcome(self) -> AppDispatchOutcome {
        AppDispatchOutcome {
            handled: self,
            publish_requested: self,
        }
    }
}

impl IntoDispatchOutcome for AppDispatchOutcome {
    fn into_dispatch_outcome(self) -> AppDispatchOutcome {
        self
    }
}

/// 把 `publish` 回调的返回值胁迫为完整发布。
///
/// 旧形状 `Result<(&UiFrame, AppStatus), String>` 保持 effects 为空的旧行为；返回
/// [`AppPublication`] 的应用让事务性 effects 随发布过线（线格式本就携带）。
pub trait IntoPublicationResult {
    /// 转换为完整发布。
    fn into_publication_result(self) -> Result<AppPublication, String>;
}

impl IntoPublicationResult for Result<AppPublication, String> {
    fn into_publication_result(self) -> Result<AppPublication, String> {
        self
    }
}

impl IntoPublicationResult for Result<(&UiFrame, AppStatus), String> {
    fn into_publication_result(self) -> Result<AppPublication, String> {
        self.and_then(|(frame, status)| {
            let token = status
                .frame_token
                .ok_or_else(|| "publication status must contain a frame token".to_owned())?;
            Ok(AppPublication {
                token,
                frame: frame.clone(),
                damage: FrameDamage::full(frame.viewport, DirtyFlags::ALL),
                spine: Vec::new(),
                retained_tree: None,
                status,
                effects: Vec::new(),
            })
        })
    }
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
///     presented = on_presented;
///     rejected = on_rejected;
/// }
/// ```
///
/// `apply` receives `(&mut App, AppEvent)` and returns either `bool`（旧语义）or
/// [`AppDispatchOutcome`]（完整协议）。`publish` receives `&mut App` and returns either
/// `Result<(&UiFrame, AppStatus), String>`（effects 为空）or
/// `Result<AppPublication, String>`（effects 随发布走）。`with_app` is the application's
/// synchronous access helper and `reset` clears its concrete application state.
///
/// 可选的 `presented` / `rejected` 尾臂把呈现回执转发回应用：
/// `presented = fn(&mut App, AppFrameToken) -> Result<AppDispatchOutcome, String>`，
/// `rejected = fn(&mut App, AppFrameToken)`。会话运行时（如
/// `tela_app_runtime::Application`）依赖 presented 提交候选帧；缺省时行为与旧宏逐字节
/// 一致。
#[macro_export]
macro_rules! export_guest {
    {
        reset = $reset:path;
        with_app = $with_app:path;
        apply = $apply:path;
        publish = $publish:path;
        $(presented = $presented:path;)?
        $(rejected = $rejected:path;)?
    } => {
        ::std::thread_local! {
            static __TELA_INPUT_BYTES: ::std::cell::RefCell<::std::vec::Vec<u8>> = const {
                ::std::cell::RefCell::new(::std::vec::Vec::new())
            };
            static __TELA_PUBLICATION_BYTES: ::std::cell::RefCell<::std::vec::Vec<u8>> = const {
                ::std::cell::RefCell::new(::std::vec::Vec::new())
            };
            static __TELA_TRANSPORT_BYTES: ::std::cell::RefCell<::std::vec::Vec<u8>> = const {
                ::std::cell::RefCell::new(::std::vec::Vec::new())
            };
            static __TELA_TRANSPORT: ::std::cell::RefCell<$crate::FrameTransportSender> =
                ::std::cell::RefCell::new($crate::FrameTransportSender::default());
            static __TELA_TRANSPORT_SEQUENCE: ::std::cell::Cell<u64> = const {
                ::std::cell::Cell::new(0)
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
            __TELA_TRANSPORT_BYTES.with(|slot| slot.borrow_mut().clear());
            __TELA_TRANSPORT.with(|slot| *slot.borrow_mut() = $crate::FrameTransportSender::default());
            __TELA_TRANSPORT_SEQUENCE.with(|slot| slot.set(0));
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
            let changed =
                $with_app(|app| $crate::IntoDispatchOutcome::into_dispatch_outcome($apply(app, event)));
            __TELA_ERROR_BYTES.with(|slot| slot.borrow_mut().clear());
            $crate::OUTCOME_OK
                | if changed.handled {
                    $crate::OUTCOME_HANDLED
                } else {
                    0
                }
                | if changed.publish_requested {
                    $crate::OUTCOME_PUBLISH_REQUESTED
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
        pub extern "C" fn tela_app_transport_ptr() -> *const u8 {
            __TELA_TRANSPORT_BYTES.with(|publication| publication.borrow().as_ptr())
        }

        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn tela_app_transport_len() -> u32 {
            __TELA_TRANSPORT_BYTES.with(|publication| publication.borrow().len() as u32)
        }

        #[allow(unsafe_code)]
        #[unsafe(no_mangle)]
        pub extern "C" fn tela_app_presented(token_low: u32, token_high: u32) -> u32 {
            let token = u64::from(token_low) | (u64::from(token_high) << 32);
            if token == 0 || __TELA_PUBLISHED_TOKEN.with(|slot| slot.get()) != token {
                __tela_set_error("presented token is not the latest publication".to_owned());
                return 0;
            }
            let mut outcome_bits = 0u32;
            $(
                let frame_token = $crate::AppFrameToken::new(token)
                    .expect("presented token is validated non-zero");
                match $with_app(|app| $presented(app, frame_token)) {
                    ::std::result::Result::Ok(outcome) => {
                        if outcome.handled {
                            outcome_bits |= $crate::OUTCOME_HANDLED;
                        }
                        if outcome.publish_requested {
                            outcome_bits |= $crate::OUTCOME_PUBLISH_REQUESTED;
                        }
                    }
                    ::std::result::Result::Err(error) => {
                        __tela_set_error(error);
                        return 0;
                    }
                }
            )?
            __TELA_PRESENTED_TOKEN.with(|slot| slot.set(token));
            __TELA_PUBLISHED_TOKEN.with(|slot| slot.set(0));
            __TELA_TRANSPORT_SEQUENCE.with(|sequence| {
                if sequence.get() != 0 {
                    __TELA_TRANSPORT.with(|sender| sender.borrow_mut().acknowledge(sequence.get()));
                    sequence.set(0);
                }
            });
            __TELA_ERROR_BYTES.with(|slot| slot.borrow_mut().clear());
            $crate::OUTCOME_OK | outcome_bits
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
            __TELA_TRANSPORT_SEQUENCE.with(|slot| {
                let sequence = slot.replace(0);
                if sequence != 0 {
                    __TELA_TRANSPORT.with(|sender| sender.borrow_mut().reject(sequence));
                }
            });
            $(
                let token = $crate::AppFrameToken::new(token)
                    .expect("rejected token is validated non-zero");
                $with_app(|app| $rejected(app, token));
            )?
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
                let publication =
                    $crate::IntoPublicationResult::into_publication_result($publish(app))?;
                let token = publication.token;
                let bytes = $crate::encode_publication(&publication)
                    .map_err(|error| error.to_string())?;
                let transport = __TELA_TRANSPORT.with(|sender| {
                    sender.borrow_mut().publish(
                        token,
                        &publication.frame,
                        &publication.damage,
                        &publication.spine,
                        publication.retained_tree.clone(),
                    )
                });
                let transport_sequence = transport.sequence();
                let transport_bytes = $crate::encode_transport_publication(
                    &$crate::TransportPublication { packet: transport, status: publication.status.clone() }
                ).map_err(|error| error.to_string())?;
                Ok::<_, ::std::string::String>((token, bytes, transport_sequence, transport_bytes))
            });
            match published {
                Ok((token, bytes, transport_sequence, transport_bytes)) => {
                    __TELA_PUBLICATION_BYTES.with(|slot| *slot.borrow_mut() = bytes);
                    __TELA_TRANSPORT_BYTES.with(|slot| *slot.borrow_mut() = transport_bytes);
                    __TELA_TRANSPORT_SEQUENCE.with(|slot| slot.set(transport_sequence));
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
/// Version 8 adds acknowledged-base transport packets with retained tree coordinates for WebView
/// guests.
/// Hosts reject bundles whose declared version does not exactly match.
pub const ABI_VERSION: u32 = 8;

#[cfg(test)]
mod coercion_tests {
    use super::*;

    fn frame() -> UiFrame {
        UiFrame {
            viewport: tela_contract::Viewport {
                width: 16.0,
                height: 8.0,
            },
            commands: Vec::new(),
            hit_regions: Vec::new(),
            scroll_bounds: Vec::new(),
        }
    }

    fn status(token: Option<AppFrameToken>) -> AppStatus {
        AppStatus {
            frame_token: token,
            cursor: CursorKind::Default,
            input_focused: false,
            input_value: String::new(),
            animation_active: false,
            next_deadline_ms: None,
        }
    }

    #[test]
    fn bool_collapses_handled_and_publish_requested_together() {
        let outcome = true.into_dispatch_outcome();
        assert!(outcome.handled && outcome.publish_requested);
        let outcome = false.into_dispatch_outcome();
        assert!(!outcome.handled && !outcome.publish_requested);
    }

    #[test]
    fn dispatch_outcome_passes_through_untouched() {
        let outcome = AppDispatchOutcome {
            handled: false,
            publish_requested: true,
        };
        assert_eq!(
            outcome.into_dispatch_outcome(),
            AppDispatchOutcome {
                handled: false,
                publish_requested: true,
            }
        );
    }

    #[test]
    fn borrowed_frame_publication_keeps_effects_empty() {
        let frame = frame();
        let publication = Ok::<_, String>((&frame, status(Some(AppFrameToken::new(1).unwrap()))))
            .into_publication_result()
            .expect("publication");
        assert_eq!(publication.token.get(), 1);
        assert!(
            publication.effects.is_empty(),
            "旧形状必须保持 effects 为空"
        );
    }

    #[test]
    fn borrowed_frame_publication_requires_a_token() {
        let frame = frame();
        let error = Ok::<_, String>((&frame, status(None)))
            .into_publication_result()
            .unwrap_err();
        assert!(error.contains("frame token"));
    }

    #[test]
    fn owned_publication_passes_through_with_effects() {
        let publication = AppPublication {
            token: AppFrameToken::new(2).unwrap(),
            frame: frame(),
            damage: FrameDamage::default(),
            spine: Vec::new(),
            retained_tree: None,
            status: status(Some(AppFrameToken::new(2).unwrap())),
            effects: vec![AppEffect::Window(tela_contract::WindowCommand::Close)],
        };
        let converted = Ok::<_, String>(publication.clone())
            .into_publication_result()
            .expect("publication");
        assert_eq!(converted, publication);
    }
}
