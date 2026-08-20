//! Semantic version triple reused across bridge capabilities, applications, and delivery bundles.

use serde::{Deserialize, Serialize};

/// Semantic version triple reused by bridge capabilities, applications, and delivery bundles.
///
/// Comparison is lexicographic with `major` taking priority. Pre-release suffixes are not
/// expressed in the triple; scenarios that need them define their own channel semantics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Version {
    /// Major version component.
    pub major: u32,
    /// Minor version component.
    pub minor: u32,
    /// Patch version component.
    pub patch: u32,
}

impl Version {
    /// Constructs a `Version` from its three components.
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordering_is_lexicographic() {
        let v = |maj: u32, min: u32, pat: u32| Version::new(maj, min, pat);
        assert!(v(1, 2, 3) < v(1, 2, 4));
        assert!(v(1, 2, 3) < v(1, 3, 0));
        assert!(v(1, 2, 3) < v(2, 0, 0));
        assert_eq!(v(2, 0, 0).max(v(1, 99, 99)), v(2, 0, 0));
    }

    #[test]
    fn round_trips_serde() {
        let version = Version::new(1, 2, 3);
        let bytes = serde_json::to_vec(&version).expect("encode");
        let decoded: Version = serde_json::from_slice(&bytes).expect("decode");
        assert_eq!(decoded, version);
    }
}
