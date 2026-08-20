//! Response payload types shared by bridge results.

use serde::{Deserialize, Serialize};
use tela_utils::Version;

/// App name payload (`std.device.getAppName`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppNameInfo {
    /// Application display name (build-time injected constant).
    pub name: String,
}

/// App semantic version payload (`std.device.getAppVersion`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppVersionInfo {
    /// Application semantic version.
    pub version: Version,
}

/// App build sequence payload (`std.device.getAppBuildId`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppBuildIdInfo {
    /// Application build sequence number.
    pub build_id: u32,
}

/// Delivery bundle semantic version payload (`std.device.getBundleVersion`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleVersionInfo {
    /// Delivery bundle semantic version.
    pub version: Version,
}

/// Delivery bundle build sequence payload (`std.device.getBundleBuildId`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleBuildIdInfo {
    /// Delivery bundle build sequence number.
    pub build_id: u32,
}

/// Wall clock and timezone snapshot payload (`std.device.getTimeStamp`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeInfo {
    /// Milliseconds since the Unix epoch. Not monotonic; never use for timing.
    pub unix_millis: u64,
    /// Current timezone offset from UTC in seconds (DST-aware); UTC+8 = `28800`.
    pub timezone_offset_seconds: i32,
    /// IANA timezone id; `"UTC"` fallback when unavailable.
    pub timezone_id: String,
}

/// Content area logical size snapshot (`std.device.getViewportSize`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewportSizeInfo {
    /// Content area logical width (safe-area deducted, same source as `AppEvent::Viewport`).
    pub width: u32,
    /// Content area logical height.
    pub height: u32,
}

/// Device pixel ratio snapshot (`std.device.getViewportDpr`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ViewportDprInfo {
    /// Device pixel ratio; physical pixels = logical pixels * dpr.
    pub dpr: f32,
}

/// Battery level payload (`std.device.getBatteryLevel`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BatteryLevelInfo {
    /// Battery level in `0.0..=1.0` (0 = empty, 1 = full).
    pub level: f32,
}

/// Charging state payload (`std.device.getBatteryCharging`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatteryChargingInfo {
    /// Whether the device is charging (including plugged and full).
    pub charging: bool,
}

/// Online state payload (`std.device.getNetworkOnline`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkOnlineInfo {
    /// Whether a network connection exists (interface level, not internet reachability).
    pub online: bool,
}

/// Network connection kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum NetworkKind {
    /// Wi-Fi connection.
    Wifi = 0,
    /// Cellular connection.
    Cellular = 1,
    /// Ethernet connection.
    Ethernet = 2,
    /// Unknown or no connection.
    Unknown = 3,
}

/// Connection kind payload (`std.device.getNetworkKind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkKindInfo {
    /// Normalized connection kind.
    pub kind: NetworkKind,
}

/// Coordinate datum (encoding) the coordinates are expressed in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum Datum {
    /// WGS84 (GPS international standard).
    Wgs84 = 0,
    /// GCJ-02 (China geodetic datum, only when the host explicitly converts).
    Gcj02 = 1,
    /// BD-09 (Baidu datum, only when the host explicitly converts).
    Bd09 = 2,
}

/// Latitude/longitude coordinates payload (`std.position.getCoordinates`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Coordinates {
    /// Latitude in degrees, interpreted in the `datum` coordinate system.
    pub latitude: f64,
    /// Longitude in degrees, interpreted in the `datum` coordinate system.
    pub longitude: f64,
    /// Horizontal accuracy in meters.
    pub accuracy_meters: f32,
    /// Wall-clock time of the fix in unix milliseconds, not the query time.
    pub timestamp_millis: u64,
    /// Coordinate datum; must always be declared.
    pub datum: Datum,
}

/// Configuration value payload (`std.config.getConfig`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigValue {
    /// Configuration value as JSON text (e.g. `"{}"`, `"true"`, `"\"dark\""`).
    pub value: String,
}

/// Success payload of `std.base.canIUse`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanIUseInfo {
    /// The host implementation version that satisfies the requested policy.
    pub hit_version: Version,
}

/// Success payload of the `ListCapabilities` sub-request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListCapabilitiesInfo {
    /// All registered capabilities with their current implementation versions.
    pub entries: Vec<crate::CapabilityEntry>,
}
