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
    AppEvent, AppStatus, CursorKind, decode_event, decode_status, encode_event, encode_status,
};
pub use frame::{WireFrame, decode_frame, encode_frame};

/// ABI version expected by the first development bundle runtime.
pub const ABI_VERSION: u32 = 1;
