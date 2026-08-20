//! Unified byte-channel bridge result.

use serde::{Deserialize, Serialize};

use crate::BridgeError;

/// Outcome of a bridge request: a capability-defined byte payload or a protocol-level error.
///
/// Success payload formats are defined per capability contract (encoded by the helper functions
/// in [`crate::payload`]). Protocol-level errors are structured so the guest can always
/// distinguish "capability missing / version mismatch / timeout" from business results.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BridgeResult {
    /// The request succeeded with the capability-defined payload bytes.
    Ok(Vec<u8>),
    /// The request failed with a protocol-level bridge error.
    Err(BridgeError),
}

impl BridgeResult {
    /// Constructs a success result.
    pub fn ok(payload: Vec<u8>) -> Self {
        Self::Ok(payload)
    }

    /// Constructs an error result.
    pub fn err(error: BridgeError) -> Self {
        Self::Err(error)
    }
}
