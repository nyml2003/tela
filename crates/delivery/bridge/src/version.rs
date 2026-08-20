//! Explicit version expectations every bridge request must carry.

use serde::{Deserialize, Serialize};
use tela_utils::Version;

/// Explicit version expectation every bridge request must carry.
///
/// There is no implicit default: the guest always declares what semantics it needs, and a
/// mismatched host must fail with [`crate::BridgeError::VersionMismatch`] instead of degrading.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VersionPolicy {
    /// Use the newest implementation; the host echoes its actual version as `hit_version`.
    Latest,
    /// Require an exact component-wise match.
    Exact(Version),
    /// Require `lower_eff <= host <= upper_eff` (closed interval).
    ///
    /// `lower: None` means `0.0.0`; `upper: None` means `255.255.255` (current cap).
    Range {
        /// Inclusive lower bound; `None` = `0.0.0`.
        lower: Option<Version>,
        /// Inclusive upper bound; `None` = `255.255.255`.
        upper: Option<Version>,
    },
}

/// Effective upper bound for an unbounded `Range`.
pub const RANGE_UPPER_CAP: Version = Version::new(255, 255, 255);

impl VersionPolicy {
    /// Returns whether `available` satisfies this policy.
    ///
    /// # Examples
    ///
    /// ```
    /// use tela_bridge::VersionPolicy;
    /// use tela_utils::Version;
    /// let at_least_1 = VersionPolicy::Range {
    ///     lower: Some(Version::new(1, 0, 0)),
    ///     upper: None,
    /// };
    /// assert!(at_least_1.matches(Version::new(1, 2, 3)));
    /// assert!(!at_least_1.matches(Version::new(0, 9, 9)));
    /// ```
    pub fn matches(self, available: Version) -> bool {
        match self {
            VersionPolicy::Latest => true,
            VersionPolicy::Exact(expected) => available == expected,
            VersionPolicy::Range { lower, upper } => {
                let lower_eff = lower.unwrap_or_default();
                let upper_eff = upper.unwrap_or(RANGE_UPPER_CAP);
                lower_eff <= available && available <= upper_eff
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(major: u32, minor: u32, patch: u32) -> Version {
        Version::new(major, minor, patch)
    }

    #[test]
    fn latest_matches_any_implementation() {
        assert!(VersionPolicy::Latest.matches(v(0, 0, 0)));
        assert!(VersionPolicy::Latest.matches(v(255, 255, 255)));
    }

    #[test]
    fn exact_requires_component_wise_equality() {
        let policy = VersionPolicy::Exact(v(1, 2, 3));
        assert!(policy.matches(v(1, 2, 3)));
        assert!(!policy.matches(v(1, 2, 4)));
        assert!(!policy.matches(v(1, 3, 3)));
        assert!(!policy.matches(v(2, 2, 3)));
    }

    #[test]
    fn range_bounds_are_inclusive() {
        let policy = VersionPolicy::Range {
            lower: Some(v(1, 0, 0)),
            upper: Some(v(2, 0, 0)),
        };
        assert!(policy.matches(v(1, 0, 0)));
        assert!(policy.matches(v(2, 0, 0)));
        assert!(policy.matches(v(1, 99, 99)));
        assert!(!policy.matches(v(0, 99, 99)));
        assert!(!policy.matches(v(2, 0, 1)));
    }

    #[test]
    fn unbounded_lower_means_zero() {
        let policy = VersionPolicy::Range {
            lower: None,
            upper: Some(v(1, 0, 0)),
        };
        assert!(policy.matches(v(0, 0, 0)));
        assert!(!policy.matches(v(1, 0, 1)));
    }

    #[test]
    fn unbounded_upper_means_255_255_255_cap() {
        let policy = VersionPolicy::Range {
            lower: Some(v(2, 0, 0)),
            upper: None,
        };
        assert!(policy.matches(v(2, 0, 0)));
        assert!(policy.matches(v(255, 255, 255)));
        assert!(!policy.matches(v(1, 99, 99)));
    }
}
