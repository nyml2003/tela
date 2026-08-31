//! Win32 bridge providers for both shells: time, viewport, battery, and network
//! (getCoordinates deferred). Single source of truth shared by the dynamic host
//! (`tela-target-win32`) and the static shell.

#![allow(unsafe_code)]

use std::cell::RefCell;
use std::rc::Rc;

use tela_bridge::{BridgeDispatcher, CapabilityId, Provider, ProviderOutcome, capabilities};
use windows::Win32::Networking::WinInet::{
    INTERNET_CONNECTION, INTERNET_CONNECTION_LAN, INTERNET_CONNECTION_MODEM,
    InternetGetConnectedState,
};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_READ, RegCloseKey, RegOpenKeyExW, RegQueryValueExW,
};
use windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime;
use windows::Win32::System::SystemServices::{TIME_ZONE_ID_DAYLIGHT, TIME_ZONE_ID_STANDARD};
use windows::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};

use tela_desktop_runtime::bridge::common::{BuildConstants, register_common_providers};

/// Logical window metrics shared with the shell (updated on every viewport dispatch).
#[derive(Clone, Copy, Debug)]
pub struct WindowMetrics {
    /// Logical client width (CSS points).
    pub width: u32,
    /// Logical client height.
    pub height: u32,
    /// Device pixel ratio.
    pub dpr: f32,
}

impl Default for WindowMetrics {
    fn default() -> Self {
        Self {
            width: 0,
            height: 0,
            dpr: 1.0,
        }
    }
}

fn immediate(payload: Vec<u8>) -> ProviderOutcome {
    ProviderOutcome::Immediate(Ok(payload))
}

fn writer_for(payload: Vec<u8>) -> ProviderOutcome {
    immediate(payload)
}

/// `std.device.getTimeStamp`: GetSystemTimeAsFileTime + GetTimeZoneInformation + registry IANA id.
pub struct Win32TimeProvider;

impl Provider for Win32TimeProvider {
    fn handle(&mut self, _payload: &[u8]) -> ProviderOutcome {
        let file_time = unsafe { GetSystemTimeAsFileTime() };
        let unix_100ns =
            (u64::from(file_time.dwHighDateTime) << 32) | u64::from(file_time.dwLowDateTime);
        let unix_millis = (unix_100ns / 10_000).saturating_sub(116_444_736_000_000);

        let mut tz = TIME_ZONE_INFORMATION::default();
        let tz_id = unsafe { GetTimeZoneInformation(&mut tz) };
        let mut offset_seconds = -(i64::from(tz.Bias)) * 60;
        if tz_id == TIME_ZONE_ID_DAYLIGHT {
            offset_seconds -= i64::from(tz.DaylightBias) * 60;
        } else if tz_id == TIME_ZONE_ID_STANDARD {
            offset_seconds -= i64::from(tz.StandardBias) * 60;
        }

        let timezone_id = read_timezone_key_name().unwrap_or_else(|| "UTC".to_owned());

        let writer = tela_bridge::encode_time_stamp_response(&tela_bridge::TimeInfo {
            unix_millis,
            timezone_offset_seconds: offset_seconds as i32,
            timezone_id,
        });
        writer_for(writer)
    }
}

/// Reads the Windows registry IANA timezone id (`TimeZoneKeyName`).
fn read_timezone_key_name() -> Option<String> {
    let mut key = HKEY::default();
    let path = windows::core::w!("SYSTEM\\CurrentControlSet\\Control\\TimeZoneInformation");
    let error = unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, path, None, KEY_READ, &mut key) };
    if error.0 != 0 {
        return None;
    }
    let mut buffer = [0u16; 128];
    let mut size = (buffer.len() as u32) * 2;
    let result = unsafe {
        RegQueryValueExW(
            key,
            windows::core::w!("TimeZoneKeyName"),
            None,
            None,
            Some(buffer.as_mut_ptr() as _),
            Some(&mut size),
        )
    };
    let _ = unsafe { RegCloseKey(key) };
    let _ = result;
    if size < 2 {
        return None;
    }
    let len = (size as usize / 2).min(buffer.len()) - 1;
    let value = String::from_utf16_lossy(&buffer[..len]);
    let _ = unsafe { RegCloseKey(key) };
    if value.is_empty() { None } else { Some(value) }
}

/// `std.device.getViewportSize` from the shared window metrics.
pub struct ViewportSizeProvider(Rc<RefCell<WindowMetrics>>);

impl Provider for ViewportSizeProvider {
    fn handle(&mut self, _payload: &[u8]) -> ProviderOutcome {
        let metrics = self.0.borrow();
        let writer = tela_bridge::encode_viewport_size_response(&tela_bridge::ViewportSizeInfo {
            width: metrics.width,
            height: metrics.height,
        });
        writer_for(writer)
    }
}

/// `std.device.getViewportDpr` from the shared window metrics.
pub struct ViewportDprProvider(Rc<RefCell<WindowMetrics>>);

impl Provider for ViewportDprProvider {
    fn handle(&mut self, _payload: &[u8]) -> ProviderOutcome {
        let metrics = self.0.borrow();
        let writer = tela_bridge::encode_viewport_dpr_response(&tela_bridge::ViewportDprInfo {
            dpr: metrics.dpr,
        });
        writer_for(writer)
    }
}

/// `std.device.getBatteryLevel` via GetSystemPowerStatus.
pub struct BatteryLevelProvider;

impl Provider for BatteryLevelProvider {
    fn handle(&mut self, _payload: &[u8]) -> ProviderOutcome {
        let mut status = SYSTEM_POWER_STATUS::default();
        let _ = unsafe { GetSystemPowerStatus(&mut status) };
        let level = match status.BatteryLifePercent {
            0..=100 => f32::from(status.BatteryLifePercent) / 100.0,
            _ => 0.0,
        };
        let writer =
            tela_bridge::encode_battery_level_response(&tela_bridge::BatteryLevelInfo { level });
        writer_for(writer)
    }
}

/// `std.device.getBatteryCharging` via GetSystemPowerStatus.
pub struct BatteryChargingProvider;

impl Provider for BatteryChargingProvider {
    fn handle(&mut self, _payload: &[u8]) -> ProviderOutcome {
        let mut status = SYSTEM_POWER_STATUS::default();
        let _ = unsafe { GetSystemPowerStatus(&mut status) };
        let charging = status.ACLineStatus == 1;
        let writer =
            tela_bridge::encode_battery_charging_response(&tela_bridge::BatteryChargingInfo {
                charging,
            });
        writer_for(writer)
    }
}

/// `std.device.getNetworkOnline` via InternetGetConnectedState.
pub struct NetworkOnlineProvider;

impl Provider for NetworkOnlineProvider {
    fn handle(&mut self, _payload: &[u8]) -> ProviderOutcome {
        let mut flags = INTERNET_CONNECTION::default();
        let online = unsafe { InternetGetConnectedState(&mut flags, None) }.is_ok();
        let writer =
            tela_bridge::encode_network_online_response(&tela_bridge::NetworkOnlineInfo { online });
        writer_for(writer)
    }
}

/// `std.device.getNetworkKind` via InternetGetConnectedState flags.
pub struct NetworkKindProvider;

impl Provider for NetworkKindProvider {
    fn handle(&mut self, _payload: &[u8]) -> ProviderOutcome {
        let mut flags = INTERNET_CONNECTION::default();
        let online = unsafe { InternetGetConnectedState(&mut flags, None) }.is_ok();
        let kind = if !online {
            3 // Unknown
        } else if flags.contains(INTERNET_CONNECTION_LAN) {
            2 // Ethernet
        } else if flags.contains(INTERNET_CONNECTION_MODEM) {
            1 // Cellular
        } else {
            3 // Unknown
        };
        let writer = tela_bridge::encode_network_kind_response(&tela_bridge::NetworkKindInfo {
            kind: match kind {
                2 => tela_bridge::NetworkKind::Ethernet,
                1 => tela_bridge::NetworkKind::Cellular,
                _ => tela_bridge::NetworkKind::Unknown,
            },
        });
        writer_for(writer)
    }
}

/// Builds the Win32 bridge dispatcher: common providers + the seven platform providers.
///
/// `getCoordinates` is deliberately not registered (deferred until a real location flow lands).
pub fn build_dispatcher(
    metrics: Rc<RefCell<WindowMetrics>>,
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
    dispatcher.register(capabilities::get_time_stamp(), Win32TimeProvider);
    dispatcher.register(
        capabilities::get_viewport_size(),
        ViewportSizeProvider(Rc::clone(&metrics)),
    );
    dispatcher.register(
        capabilities::get_viewport_dpr(),
        ViewportDprProvider(Rc::clone(&metrics)),
    );
    dispatcher.register(capabilities::get_battery_level(), BatteryLevelProvider);
    dispatcher.register(
        capabilities::get_battery_charging(),
        BatteryChargingProvider,
    );
    dispatcher.register(capabilities::get_network_online(), NetworkOnlineProvider);
    dispatcher.register(capabilities::get_network_kind(), NetworkKindProvider);
    dispatcher
}

#[cfg(all(test, target_os = "windows"))]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use tela_bridge::{BridgeEvent, BridgeRequest, BridgeResult, VersionPolicy};

    #[test]
    fn dispatcher_registers_only_implemented_capabilities() {
        let metrics = Rc::new(RefCell::new(WindowMetrics {
            width: 800,
            height: 600,
            dpr: 1.5,
        }));
        let mut dispatcher = build_dispatcher(metrics, &BuildConstants::default(), vec![]);
        // 已注册的 viewportSize 必须响应。
        let event = dispatcher
            .handle(BridgeRequest::new(
                1,
                VersionPolicy::Latest,
                capabilities::get_viewport_size(),
            ))
            .expect("immediate");
        match event {
            BridgeEvent::Response {
                result: BridgeResult::Ok(bytes),
                ..
            } => {
                let info = tela_bridge::decode_viewport_size_response(&bytes).expect("decode");
                assert_eq!((info.width, info.height), (800, 600));
            }
            other => panic!("unexpected {other:?}"),
        }
        // 未注册的 getCoordinates 必须回 UnknownCapability。
        let event = dispatcher
            .handle(BridgeRequest::new(
                2,
                VersionPolicy::Latest,
                capabilities::get_coordinates(),
            ))
            .expect("immediate");
        match event {
            BridgeEvent::Response {
                result: BridgeResult::Err(BridgeError::UnknownCapability),
                ..
            } => {}
            other => panic!("unexpected {other:?}"),
        }
    }
}
