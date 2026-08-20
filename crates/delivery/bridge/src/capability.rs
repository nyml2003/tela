//! Capability identification for the unified external-contract model: `scope.group.name`.

use std::fmt;

use serde::{Deserialize, Serialize};

use tela_utils::Version;

/// Capability scope: the first dimension of every capability id.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CapabilityScope {
    /// `std`: the cross-end contract published by tela (all targets commit to implement).
    Std,
    /// A named scope registered by its implementer (target-specific or host business).
    Named(String),
}

/// A bridge capability identified by scope, functional group, and atomic capability name.
///
/// Every capability — `std` published contracts, target-specific bridges, and host business
/// bridges — is an external contract item addressed identically: discovery via canIUse, version
/// negotiation, and a unified byte payload channel.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CapabilityId {
    /// Capability scope (`std` or a named scope).
    pub scope: CapabilityScope,
    /// Functional group, e.g. `"device"`.
    pub group: String,
    /// Atomic capability name, e.g. `"getBatteryLevel"`.
    pub name: String,
}

impl CapabilityId {
    /// Constructs a `std` capability id from its group and name.
    pub fn std(group: &str, name: &str) -> Self {
        Self {
            scope: CapabilityScope::Std,
            group: group.to_owned(),
            name: name.to_owned(),
        }
    }

    /// Constructs a named-scope capability id (target-specific or host business).
    pub fn named(scope: impl Into<String>, group: &str, name: &str) -> Self {
        Self {
            scope: CapabilityScope::Named(scope.into()),
            group: group.to_owned(),
            name: name.to_owned(),
        }
    }
}

impl fmt::Display for CapabilityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.scope {
            CapabilityScope::Std => write!(f, "std.{}.{}", self.group, self.name),
            CapabilityScope::Named(scope) => write!(f, "{scope}.{}.{}", self.group, self.name),
        }
    }
}

/// One capability entry reported by `ListCapabilities`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityEntry {
    /// The registered capability.
    pub capability: CapabilityId,
    /// The host implementation version that currently satisfies `Latest`.
    pub hit_version: Version,
}

/// Typed capability constants for the MVP read-only `std` bridges.
pub mod capabilities {
    use super::CapabilityId;

    /// `std.base.canIUse`: capability discovery and version negotiation.
    pub fn can_i_use() -> CapabilityId {
        CapabilityId::std("base", "canIUse")
    }
    /// `std.device.getAppName`: application display name.
    pub fn get_app_name() -> CapabilityId {
        CapabilityId::std("device", "getAppName")
    }
    /// `std.device.getAppVersion`: application semantic version.
    pub fn get_app_version() -> CapabilityId {
        CapabilityId::std("device", "getAppVersion")
    }
    /// `std.device.getAppBuildId`: application build sequence number.
    pub fn get_app_build_id() -> CapabilityId {
        CapabilityId::std("device", "getAppBuildId")
    }
    /// `std.device.getBundleVersion`: delivery bundle semantic version.
    pub fn get_bundle_version() -> CapabilityId {
        CapabilityId::std("device", "getBundleVersion")
    }
    /// `std.device.getBundleBuildId`: delivery bundle build sequence number.
    pub fn get_bundle_build_id() -> CapabilityId {
        CapabilityId::std("device", "getBundleBuildId")
    }
    /// `std.device.getTimeStamp`: wall clock and timezone snapshot.
    pub fn get_time_stamp() -> CapabilityId {
        CapabilityId::std("device", "getTimeStamp")
    }
    /// `std.device.getViewportSize`: content area logical size snapshot.
    pub fn get_viewport_size() -> CapabilityId {
        CapabilityId::std("device", "getViewportSize")
    }
    /// `std.device.getViewportDpr`: device pixel ratio snapshot.
    pub fn get_viewport_dpr() -> CapabilityId {
        CapabilityId::std("device", "getViewportDpr")
    }
    /// `std.device.getBatteryLevel`: battery level (0.0-1.0).
    pub fn get_battery_level() -> CapabilityId {
        CapabilityId::std("device", "getBatteryLevel")
    }
    /// `std.device.getBatteryCharging`: charging state.
    pub fn get_battery_charging() -> CapabilityId {
        CapabilityId::std("device", "getBatteryCharging")
    }
    /// `std.device.getNetworkOnline`: online state.
    pub fn get_network_online() -> CapabilityId {
        CapabilityId::std("device", "getNetworkOnline")
    }
    /// `std.device.getNetworkKind`: connection kind.
    pub fn get_network_kind() -> CapabilityId {
        CapabilityId::std("device", "getNetworkKind")
    }
    /// `std.position.getCoordinates`: current latitude/longitude coordinates.
    pub fn get_coordinates() -> CapabilityId {
        CapabilityId::std("position", "getCoordinates")
    }
    /// `std.config.getConfig`: read a configuration value by key.
    pub fn get_config() -> CapabilityId {
        CapabilityId::std("config", "getConfig")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_renders_two_level_std_id() {
        assert_eq!(
            capabilities::get_battery_level().to_string(),
            "std.device.getBatteryLevel"
        );
    }

    #[test]
    fn display_renders_named_scope() {
        assert_eq!(
            CapabilityId::named("shop", "cart", "getCount").to_string(),
            "shop.cart.getCount"
        );
    }

    #[test]
    fn structured_fields_serialize_without_parsing() {
        let id = CapabilityId::named("web", "storage", "getValue");
        let bytes = postcard::to_allocvec(&id).expect("encode");
        let decoded: CapabilityId = postcard::from_bytes(&bytes).expect("decode");
        assert_eq!(decoded, id);
    }

    #[test]
    fn mvp_capability_constants_are_distinct() {
        let ids = [
            capabilities::can_i_use(),
            capabilities::get_app_name(),
            capabilities::get_app_version(),
            capabilities::get_app_build_id(),
            capabilities::get_bundle_version(),
            capabilities::get_bundle_build_id(),
            capabilities::get_time_stamp(),
            capabilities::get_viewport_size(),
            capabilities::get_viewport_dpr(),
            capabilities::get_battery_level(),
            capabilities::get_battery_charging(),
            capabilities::get_network_online(),
            capabilities::get_network_kind(),
            capabilities::get_coordinates(),
            capabilities::get_config(),
        ];
        for (index, left) in ids.iter().enumerate() {
            for right in &ids[index + 1..] {
                assert_ne!(left, right, "{left} duplicates another capability");
            }
        }
    }
}
