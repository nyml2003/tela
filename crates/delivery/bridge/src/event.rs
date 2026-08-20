//! Host-to-guest bridge events.

use serde::{Deserialize, Serialize};

use crate::BridgeResult;

/// A bridge event delivered to the guest via `tela_app_bridge_dispatch`.
///
/// MVP carries only `Response`; `Message` (pubsub push) is reserved and intentionally not
/// defined here until the message bus regresses.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum BridgeEvent {
    /// The correlated response to a guest request. May arrive on any frame.
    Response {
        /// The request id the guest assigned when enqueuing the request.
        request_id: u64,
        /// The request outcome.
        result: BridgeResult,
    },
}
