//! Per-capability payload encoders/decoders for the unified byte channel.
//!
//! The wire carries only `CapabilityId` + payload bytes; these helpers define the payload
//! formats of the `std` bridges so guest and host keep type safety at the call site without the
//! protocol carrying typed enums.

use std::fmt;

use serde::{Serialize, de::DeserializeOwned};

use crate::model::{
    AppBuildIdInfo, AppNameInfo, AppVersionInfo, BatteryChargingInfo, BatteryLevelInfo,
    BundleBuildIdInfo, BundleVersionInfo, ConfigValue, Coordinates, NetworkKindInfo,
    NetworkOnlineInfo, TimeInfo, ViewportDprInfo, ViewportSizeInfo,
};
use crate::{CanIUseInfo, CapabilityId, ListCapabilitiesInfo};

/// A payload could not be encoded or decoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PayloadError {
    /// Encoding failed.
    Encode(String),
    /// Decoding failed.
    Decode(String),
}

impl fmt::Display for PayloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Encode(error) => write!(f, "could not encode bridge payload: {error}"),
            Self::Decode(error) => write!(f, "could not decode bridge payload: {error}"),
        }
    }
}

impl std::error::Error for PayloadError {}

fn encode<T: Serialize>(value: &T) -> Vec<u8> {
    postcard::to_allocvec(value).expect("encode bridge payload")
}

fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, PayloadError> {
    postcard::from_bytes(bytes).map_err(|error| PayloadError::Decode(error.to_string()))
}

// ---------------------------------------------------------------------------
// Request payloads (only capabilities with parameters need encoders).
// ---------------------------------------------------------------------------

/// Encodes a `canIUse` request payload: the target capability id.
pub fn encode_can_i_use_request(capability: &CapabilityId) -> Vec<u8> {
    encode(capability)
}

/// Decodes a `canIUse` request payload.
pub fn decode_can_i_use_request(bytes: &[u8]) -> Result<CapabilityId, PayloadError> {
    decode(bytes)
}

/// Encodes a `getConfig` request payload: the configuration key.
pub fn encode_get_config_request(key: &str) -> Vec<u8> {
    encode(&key.to_owned())
}

/// Decodes a `getConfig` request payload.
pub fn decode_get_config_request(bytes: &[u8]) -> Result<String, PayloadError> {
    decode(bytes)
}

// ---------------------------------------------------------------------------
// Response payloads (one pair per `std` bridge).
// ---------------------------------------------------------------------------

/// Encodes a `canIUse` response payload.
pub fn encode_can_i_use_response(info: &CanIUseInfo) -> Vec<u8> {
    encode(info)
}

/// Decodes a `canIUse` response payload.
pub fn decode_can_i_use_response(bytes: &[u8]) -> Result<CanIUseInfo, PayloadError> {
    decode(bytes)
}

/// Encodes a `ListCapabilities` response payload.
pub fn encode_list_capabilities_response(info: &ListCapabilitiesInfo) -> Vec<u8> {
    encode(info)
}

/// Decodes a `ListCapabilities` response payload.
pub fn decode_list_capabilities_response(
    bytes: &[u8],
) -> Result<ListCapabilitiesInfo, PayloadError> {
    decode(bytes)
}

/// Encodes a `getAppName` response payload.
pub fn encode_app_name_response(info: &AppNameInfo) -> Vec<u8> {
    encode(info)
}

/// Decodes a `getAppName` response payload.
pub fn decode_app_name_response(bytes: &[u8]) -> Result<AppNameInfo, PayloadError> {
    decode(bytes)
}

/// Encodes a `getAppVersion` response payload.
pub fn encode_app_version_response(info: &AppVersionInfo) -> Vec<u8> {
    encode(info)
}

/// Decodes a `getAppVersion` response payload.
pub fn decode_app_version_response(bytes: &[u8]) -> Result<AppVersionInfo, PayloadError> {
    decode(bytes)
}

/// Encodes a `getAppBuildId` response payload.
pub fn encode_app_build_id_response(info: &AppBuildIdInfo) -> Vec<u8> {
    encode(info)
}

/// Decodes a `getAppBuildId` response payload.
pub fn decode_app_build_id_response(bytes: &[u8]) -> Result<AppBuildIdInfo, PayloadError> {
    decode(bytes)
}

/// Encodes a `getBundleVersion` response payload.
pub fn encode_bundle_version_response(info: &BundleVersionInfo) -> Vec<u8> {
    encode(info)
}

/// Decodes a `getBundleVersion` response payload.
pub fn decode_bundle_version_response(bytes: &[u8]) -> Result<BundleVersionInfo, PayloadError> {
    decode(bytes)
}

/// Encodes a `getBundleBuildId` response payload.
pub fn encode_bundle_build_id_response(info: &BundleBuildIdInfo) -> Vec<u8> {
    encode(info)
}

/// Decodes a `getBundleBuildId` response payload.
pub fn decode_bundle_build_id_response(bytes: &[u8]) -> Result<BundleBuildIdInfo, PayloadError> {
    decode(bytes)
}

/// Encodes a `getTimeStamp` response payload.
pub fn encode_time_stamp_response(info: &TimeInfo) -> Vec<u8> {
    encode(info)
}

/// Decodes a `getTimeStamp` response payload.
pub fn decode_time_stamp_response(bytes: &[u8]) -> Result<TimeInfo, PayloadError> {
    decode(bytes)
}

/// Encodes a `getViewportSize` response payload.
pub fn encode_viewport_size_response(info: &ViewportSizeInfo) -> Vec<u8> {
    encode(info)
}

/// Decodes a `getViewportSize` response payload.
pub fn decode_viewport_size_response(bytes: &[u8]) -> Result<ViewportSizeInfo, PayloadError> {
    decode(bytes)
}

/// Encodes a `getViewportDpr` response payload.
pub fn encode_viewport_dpr_response(info: &ViewportDprInfo) -> Vec<u8> {
    encode(info)
}

/// Decodes a `getViewportDpr` response payload.
pub fn decode_viewport_dpr_response(bytes: &[u8]) -> Result<ViewportDprInfo, PayloadError> {
    decode(bytes)
}

/// Encodes a `getBatteryLevel` response payload.
pub fn encode_battery_level_response(info: &BatteryLevelInfo) -> Vec<u8> {
    encode(info)
}

/// Decodes a `getBatteryLevel` response payload.
pub fn decode_battery_level_response(bytes: &[u8]) -> Result<BatteryLevelInfo, PayloadError> {
    decode(bytes)
}

/// Encodes a `getBatteryCharging` response payload.
pub fn encode_battery_charging_response(info: &BatteryChargingInfo) -> Vec<u8> {
    encode(info)
}

/// Decodes a `getBatteryCharging` response payload.
pub fn decode_battery_charging_response(bytes: &[u8]) -> Result<BatteryChargingInfo, PayloadError> {
    decode(bytes)
}

/// Encodes a `getNetworkOnline` response payload.
pub fn encode_network_online_response(info: &NetworkOnlineInfo) -> Vec<u8> {
    encode(info)
}

/// Decodes a `getNetworkOnline` response payload.
pub fn decode_network_online_response(bytes: &[u8]) -> Result<NetworkOnlineInfo, PayloadError> {
    decode(bytes)
}

/// Encodes a `getNetworkKind` response payload.
pub fn encode_network_kind_response(info: &NetworkKindInfo) -> Vec<u8> {
    encode(info)
}

/// Decodes a `getNetworkKind` response payload.
pub fn decode_network_kind_response(bytes: &[u8]) -> Result<NetworkKindInfo, PayloadError> {
    decode(bytes)
}

/// Encodes a `getCoordinates` response payload.
pub fn encode_coordinates_response(info: &Coordinates) -> Vec<u8> {
    encode(info)
}

/// Decodes a `getCoordinates` response payload.
pub fn decode_coordinates_response(bytes: &[u8]) -> Result<Coordinates, PayloadError> {
    decode(bytes)
}

/// Encodes a `getConfig` response payload.
pub fn encode_config_response(info: &ConfigValue) -> Vec<u8> {
    encode(info)
}

/// Decodes a `getConfig` response payload.
pub fn decode_config_response(bytes: &[u8]) -> Result<ConfigValue, PayloadError> {
    decode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Datum;
    use tela_utils::Version;

    #[test]
    fn can_i_use_request_round_trips_capability() {
        let capability = crate::capabilities::get_battery_level();
        let payload = encode_can_i_use_request(&capability);
        assert_eq!(
            decode_can_i_use_request(&payload).expect("decode"),
            capability
        );
    }

    #[test]
    fn config_request_round_trips_key() {
        let payload = encode_get_config_request("app.theme");
        assert_eq!(
            decode_get_config_request(&payload).expect("decode"),
            "app.theme"
        );
    }

    #[test]
    fn all_response_payloads_round_trip() {
        let pairs: Vec<(&str, Vec<u8>)> = vec![
            (
                "canIUse",
                encode_can_i_use_response(&CanIUseInfo {
                    hit_version: Version::new(1, 0, 0),
                }),
            ),
            (
                "list",
                encode_list_capabilities_response(&ListCapabilitiesInfo { entries: vec![] }),
            ),
            (
                "appName",
                encode_app_name_response(&AppNameInfo {
                    name: "demo".into(),
                }),
            ),
            (
                "appVersion",
                encode_app_version_response(&AppVersionInfo {
                    version: Version::new(1, 2, 3),
                }),
            ),
            (
                "appBuildId",
                encode_app_build_id_response(&AppBuildIdInfo { build_id: 42 }),
            ),
            (
                "bundleVersion",
                encode_bundle_version_response(&BundleVersionInfo {
                    version: Version::new(2, 0, 0),
                }),
            ),
            (
                "bundleBuildId",
                encode_bundle_build_id_response(&BundleBuildIdInfo { build_id: 7 }),
            ),
            (
                "timeStamp",
                encode_time_stamp_response(&TimeInfo {
                    unix_millis: 1_700_000_000_000,
                    timezone_offset_seconds: 28_800,
                    timezone_id: "Asia/Shanghai".into(),
                }),
            ),
            (
                "viewportSize",
                encode_viewport_size_response(&ViewportSizeInfo {
                    width: 640,
                    height: 480,
                }),
            ),
            (
                "viewportDpr",
                encode_viewport_dpr_response(&ViewportDprInfo { dpr: 2.0 }),
            ),
            (
                "batteryLevel",
                encode_battery_level_response(&BatteryLevelInfo { level: 0.87 }),
            ),
            (
                "batteryCharging",
                encode_battery_charging_response(&BatteryChargingInfo { charging: true }),
            ),
            (
                "networkOnline",
                encode_network_online_response(&NetworkOnlineInfo { online: true }),
            ),
            (
                "networkKind",
                encode_network_kind_response(&NetworkKindInfo {
                    kind: crate::NetworkKind::Wifi,
                }),
            ),
            (
                "coordinates",
                encode_coordinates_response(&Coordinates {
                    latitude: 31.23,
                    longitude: 121.47,
                    accuracy_meters: 12.5,
                    timestamp_millis: 1_700_000_000_000,
                    datum: Datum::Wgs84,
                }),
            ),
            (
                "config",
                encode_config_response(&ConfigValue {
                    value: r#"{"theme":"dark"}"#.into(),
                }),
            ),
        ];
        assert_eq!(pairs.len(), 16);
        for (name, payload) in pairs {
            let decoded_ok: bool = match name {
                "canIUse" => decode_can_i_use_response(&payload).is_ok(),
                "list" => decode_list_capabilities_response(&payload).is_ok(),
                "appName" => decode_app_name_response(&payload).is_ok(),
                "appVersion" => decode_app_version_response(&payload).is_ok(),
                "appBuildId" => decode_app_build_id_response(&payload).is_ok(),
                "bundleVersion" => decode_bundle_version_response(&payload).is_ok(),
                "bundleBuildId" => decode_bundle_build_id_response(&payload).is_ok(),
                "timeStamp" => decode_time_stamp_response(&payload).is_ok(),
                "viewportSize" => decode_viewport_size_response(&payload).is_ok(),
                "viewportDpr" => decode_viewport_dpr_response(&payload).is_ok(),
                "batteryLevel" => decode_battery_level_response(&payload).is_ok(),
                "batteryCharging" => decode_battery_charging_response(&payload).is_ok(),
                "networkOnline" => decode_network_online_response(&payload).is_ok(),
                "networkKind" => decode_network_kind_response(&payload).is_ok(),
                "coordinates" => decode_coordinates_response(&payload).is_ok(),
                "config" => decode_config_response(&payload).is_ok(),
                other => panic!("unexpected {other}"),
            };
            assert!(decoded_ok, "{name} payload failed to decode");
        }
    }

    #[test]
    fn decoding_garbage_fails() {
        assert!(decode_app_name_response(b"not a payload").is_err());
        assert!(decode_coordinates_response(&[]).is_err());
    }
}
