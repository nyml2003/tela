//! Unified byte-channel bridge request envelope.
//!
//! Every capability — `std` published contracts, target-specific bridges, and host business
//! bridges — is addressed identically: a `CapabilityId` plus a capability-defined byte payload.
//! The payload formats of the `std` bridges are specified in the contract docs and encoded by
//! the helper functions in [`crate::payload`].

use serde::{Deserialize, Serialize};

use crate::{CapabilityId, VersionPolicy};

/// Unified envelope of a guest-to-host bridge request.
///
/// The guest assigns `request_id`; the host echoes it in `BridgeEvent::Response`. Responses may
/// arrive on any frame (unified async semantics), so the guest must never assume same-round
/// delivery.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeRequest {
    /// Guest-assigned request id correlating the response.
    pub request_id: u64,
    /// Explicit version expectation every request carries.
    pub version: VersionPolicy,
    /// The addressed capability (any scope: `std`, target-specific, or host business).
    pub capability: CapabilityId,
    /// Capability-defined request payload bytes (format per capability contract).
    pub payload: Vec<u8>,
}

impl BridgeRequest {
    /// Constructs a request envelope for `capability` with an empty payload.
    pub fn new(request_id: u64, version: VersionPolicy, capability: CapabilityId) -> Self {
        Self {
            request_id,
            version,
            capability,
            payload: Vec::new(),
        }
    }

    /// Constructs a request envelope with an explicit payload.
    pub fn with_payload(
        request_id: u64,
        version: VersionPolicy,
        capability: CapabilityId,
        payload: Vec<u8>,
    ) -> Self {
        Self {
            request_id,
            version,
            capability,
            payload,
        }
    }
}

/// Convenience builder for a request with `Latest` policy and an empty payload.
pub fn request_latest(request_id: u64, capability: CapabilityId) -> BridgeRequest {
    BridgeRequest::new(request_id, VersionPolicy::Latest, capability)
}

/// Convenience builder for a request with an explicit version policy and empty payload.
pub fn request_with_policy(
    request_id: u64,
    version: VersionPolicy,
    capability: CapabilityId,
) -> BridgeRequest {
    BridgeRequest::new(request_id, version, capability)
}

/// Convenience builder for a request with an explicit policy and payload.
pub fn request_with_payload(
    request_id: u64,
    version: VersionPolicy,
    capability: CapabilityId,
    payload: Vec<u8>,
) -> BridgeRequest {
    BridgeRequest::with_payload(request_id, version, capability, payload)
}
