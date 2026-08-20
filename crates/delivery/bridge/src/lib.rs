//! Versioned contract for the Tela host bridge.
//!
//! The bridge lets a guest request capabilities from its host. Guest and host exchange only
//! versioned byte packets over the same boundary rules as `tela_app_abi`: no Rust references,
//! trait objects, or platform handles cross the wire.
//!
//! Unified external-contract model: every capability — `std` published contracts,
//! target-specific bridges, and host business bridges — is addressed identically
//! (`CapabilityId` + payload bytes), discovered via `canIUse`, and executed through the same
//! host registry. Guests are external implementers: the four export functions are a public ABI
//! any language can implement, and the `GuestBridge` facade is a reference implementation for
//! first-party sources.
//!
//! Design authority: `docs/桥/000-宿主桥总览.md`, `docs/桥/通用模型/README.md`, and
//! `docs/032-宿主桥MVP实施目标.md`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod capability;
mod codec;
mod error;
mod event;
mod model;
mod payload;
mod request;
mod result;
mod version;

pub use capability::{CapabilityEntry, CapabilityId, CapabilityScope, capabilities};
pub use codec::{
    BridgeCodecError, decode_event, decode_request, decode_request_stream, encode_event,
    encode_request,
};
pub use error::BridgeError;
pub use event::BridgeEvent;
pub use model::{
    AppBuildIdInfo, AppNameInfo, AppVersionInfo, BatteryChargingInfo, BatteryLevelInfo,
    BundleBuildIdInfo, BundleVersionInfo, CanIUseInfo, ConfigValue, Coordinates, Datum,
    ListCapabilitiesInfo, NetworkKind, NetworkKindInfo, NetworkOnlineInfo, TimeInfo,
    ViewportDprInfo, ViewportSizeInfo,
};
pub use payload::*;
pub use request::{BridgeRequest, request_latest, request_with_payload, request_with_policy};
pub use result::BridgeResult;
pub use version::{RANGE_UPPER_CAP, VersionPolicy};

#[cfg(feature = "guest")]
mod guest;
#[cfg(feature = "guest")]
pub use guest::GuestBridge;

#[cfg(feature = "host")]
mod host;
#[cfg(feature = "host")]
pub use host::{BridgeDispatcher, Provider, ProviderOutcome};
