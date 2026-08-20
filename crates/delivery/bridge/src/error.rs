//! Bridge-level errors exchanged between guest and host.

use serde::{Deserialize, Serialize};

use crate::VersionPolicy;
use tela_utils::Version;

/// Complete error set for the host bridge.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BridgeError {
    /// The capability is unknown to the contract (reserved or unregistered group/name).
    UnknownCapability,
    /// The requested [`VersionPolicy`] is not satisfied by the host implementation.
    VersionMismatch {
        /// The policy the guest requested.
        policy: VersionPolicy,
        /// The host's actual implementation version.
        available: Version,
    },
    /// Permission denied by the platform (e.g. location permission flow).
    PermissionDenied,
    /// A configuration key does not exist.
    KeyNotFound,
    /// The host execution timed out (defensive; MVP hosts may not implement it).
    Timeout,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_mismatch_round_trips_with_policy() {
        let error = BridgeError::VersionMismatch {
            policy: VersionPolicy::Exact(Version::new(2, 0, 0)),
            available: Version::new(1, 0, 0),
        };
        let bytes = postcard::to_allocvec(&error).expect("encode");
        let decoded: BridgeError = postcard::from_bytes(&bytes).expect("decode");
        assert_eq!(decoded, error);
    }

    #[test]
    fn all_variants_round_trip() {
        for error in [
            BridgeError::UnknownCapability,
            BridgeError::PermissionDenied,
            BridgeError::KeyNotFound,
            BridgeError::Timeout,
            BridgeError::VersionMismatch {
                policy: VersionPolicy::Latest,
                available: Version::new(1, 1, 1),
            },
        ] {
            let bytes = postcard::to_allocvec(&error).expect("encode");
            let decoded: BridgeError = postcard::from_bytes(&bytes).expect("decode");
            assert_eq!(decoded, error);
        }
    }
}
