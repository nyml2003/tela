//! macOS bridge providers: time (Foundation), viewport (shared metrics), battery (IOKit FFI),
//! and network (SCNetworkReachability FFI). `getCoordinates` is deferred.

use std::cell::RefCell;
use std::rc::Rc;

use objc2_foundation::{NSDate, NSTimeZone};
use tela_bridge::{BridgeDispatcher, CapabilityId, Provider, ProviderOutcome, capabilities};

use tela_desktop_runtime::bridge::common::{BuildConstants, register_common_providers};

use crate::ffi::{battery_charging, battery_level, network_reachable};

/// Logical window metrics shared with the shell (updated on every viewport dispatch).
#[derive(Clone, Copy, Debug)]
pub struct MacMetrics {
    /// Logical content width (points).
    pub width: u32,
    /// Logical content height.
    pub height: u32,
    /// Backing scale factor.
    pub dpr: f32,
}

impl Default for MacMetrics {
    fn default() -> Self {
        Self {
            width: 0,
            height: 0,
            dpr: 2.0,
        }
    }
}

fn immediate(payload: Vec<u8>) -> ProviderOutcome {
    ProviderOutcome::Immediate(Ok(payload))
}

/// `std.device.getTimeStamp`: NSDate + NSTimeZone (DST-aware offset, IANA id).
pub struct FoundationTimeProvider;

impl Provider for FoundationTimeProvider {
    fn handle(&mut self, _payload: &[u8]) -> ProviderOutcome {
        let now = NSDate::now();
        let unix_millis = (now.timeIntervalSince1970() * 1000.0).max(0.0) as u64;
        let timezone = NSTimeZone::localTimeZone();
        let offset_seconds = timezone.secondsFromGMT();
        let name = timezone.name().to_string();
        let writer = tela_bridge::encode_time_stamp_response(&tela_bridge::TimeInfo {
            unix_millis,
            timezone_offset_seconds: offset_seconds as i32,
            timezone_id: if name.is_empty() {
                "UTC".to_owned()
            } else {
                name
            },
        });
        immediate(writer)
    }
}

/// `std.device.getViewportSize` from shared metrics.
pub struct ViewportSizeProvider(Rc<RefCell<MacMetrics>>);

impl Provider for ViewportSizeProvider {
    fn handle(&mut self, _payload: &[u8]) -> ProviderOutcome {
        let metrics = self.0.borrow();
        immediate(tela_bridge::encode_viewport_size_response(
            &tela_bridge::ViewportSizeInfo {
                width: metrics.width,
                height: metrics.height,
            },
        ))
    }
}

/// `std.device.getViewportDpr` from shared metrics.
pub struct ViewportDprProvider(Rc<RefCell<MacMetrics>>);

impl Provider for ViewportDprProvider {
    fn handle(&mut self, _payload: &[u8]) -> ProviderOutcome {
        let metrics = self.0.borrow();
        immediate(tela_bridge::encode_viewport_dpr_response(
            &tela_bridge::ViewportDprInfo { dpr: metrics.dpr },
        ))
    }
}

/// `std.device.getBatteryLevel` via IOKit power sources.
pub struct IokitBatteryLevelProvider;

impl Provider for IokitBatteryLevelProvider {
    fn handle(&mut self, _payload: &[u8]) -> ProviderOutcome {
        let level = battery_level().unwrap_or(0.0);
        immediate(tela_bridge::encode_battery_level_response(
            &tela_bridge::BatteryLevelInfo { level },
        ))
    }
}

/// `std.device.getBatteryCharging` via IOKit power sources.
pub struct IokitBatteryChargingProvider;

impl Provider for IokitBatteryChargingProvider {
    fn handle(&mut self, _payload: &[u8]) -> ProviderOutcome {
        let charging = battery_charging().unwrap_or(false);
        immediate(tela_bridge::encode_battery_charging_response(
            &tela_bridge::BatteryChargingInfo { charging },
        ))
    }
}

/// `std.device.getNetworkOnline` via SCNetworkReachability.
pub struct ReachabilityOnlineProvider;

impl Provider for ReachabilityOnlineProvider {
    fn handle(&mut self, _payload: &[u8]) -> ProviderOutcome {
        immediate(tela_bridge::encode_network_online_response(
            &tela_bridge::NetworkOnlineInfo {
                online: network_reachable(),
            },
        ))
    }
}

/// `std.device.getNetworkKind`: macOS reachability does not expose a connection kind; the
/// provider reports Ethernet when reachable and Unknown otherwise.
pub struct ReachabilityKindProvider;

impl Provider for ReachabilityKindProvider {
    fn handle(&mut self, _payload: &[u8]) -> ProviderOutcome {
        let kind = if network_reachable() {
            tela_bridge::NetworkKind::Ethernet
        } else {
            tela_bridge::NetworkKind::Unknown
        };
        immediate(tela_bridge::encode_network_kind_response(
            &tela_bridge::NetworkKindInfo { kind },
        ))
    }
}

/// Builds the macOS bridge dispatcher: common providers plus the seven platform providers.
///
/// `getCoordinates` is deliberately not registered (CoreLocation flow deferred).
pub fn build_dispatcher(
    metrics: Rc<RefCell<MacMetrics>>,
    build: &BuildConstants,
    config: Vec<(String, String)>,
) -> BridgeDispatcher {
    let mut dispatcher = BridgeDispatcher::new();
    let implemented: Vec<CapabilityId> = vec![
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
        capabilities::get_config(),
    ];
    register_common_providers(&mut dispatcher, build, config, &implemented);
    dispatcher.register(capabilities::get_time_stamp(), FoundationTimeProvider);
    dispatcher.register(
        capabilities::get_viewport_size(),
        ViewportSizeProvider(Rc::clone(&metrics)),
    );
    dispatcher.register(
        capabilities::get_viewport_dpr(),
        ViewportDprProvider(Rc::clone(&metrics)),
    );
    dispatcher.register(capabilities::get_battery_level(), IokitBatteryLevelProvider);
    dispatcher.register(
        capabilities::get_battery_charging(),
        IokitBatteryChargingProvider,
    );
    dispatcher.register(
        capabilities::get_network_online(),
        ReachabilityOnlineProvider,
    );
    dispatcher.register(capabilities::get_network_kind(), ReachabilityKindProvider);
    dispatcher
}
