//! Platform-neutral desktop bridge providers: build-time constant bridges, the canIUse static
//! table, and the config provider. Platform providers register alongside these.

use tela_bridge::{BridgeError, CapabilityEntry, CapabilityId, Provider, ProviderOutcome};
use tela_utils::Version;

/// Build-time injected constants backing the version bridges.
///
/// The desktop shell injects these at build time; current development defaults are the
/// placeholder values until the build pipeline wires real constants.
#[derive(Clone, Debug)]
pub struct BuildConstants {
    /// Application display name.
    pub app_name: String,
    /// Application semantic version.
    pub app_version: Version,
    /// Application build sequence number.
    pub app_build_id: u32,
    /// Delivery bundle semantic version.
    pub bundle_version: Version,
    /// Delivery bundle build sequence number.
    pub bundle_build_id: u32,
}

impl Default for BuildConstants {
    fn default() -> Self {
        Self {
            app_name: "Tela 桌面".to_owned(),
            app_version: Version::new(0, 1, 0),
            app_build_id: 1,
            bundle_version: Version::new(0, 1, 0),
            bundle_build_id: 1,
        }
    }
}

fn immediate(payload: Vec<u8>) -> ProviderOutcome {
    ProviderOutcome::Immediate(Ok(payload))
}

/// App name from build constants.
pub struct BuildAppNameProvider(pub String);

impl Provider for BuildAppNameProvider {
    fn handle(&mut self, _payload: &[u8]) -> ProviderOutcome {
        immediate(tela_bridge::encode_app_name_response(
            &tela_bridge::AppNameInfo {
                name: self.0.clone(),
            },
        ))
    }
}

/// App version from build constants.
pub struct BuildAppVersionProvider(pub Version);

impl Provider for BuildAppVersionProvider {
    fn handle(&mut self, _payload: &[u8]) -> ProviderOutcome {
        immediate(tela_bridge::encode_app_version_response(
            &tela_bridge::AppVersionInfo { version: self.0 },
        ))
    }
}

/// App build id from build constants.
pub struct BuildAppBuildIdProvider(pub u32);

impl Provider for BuildAppBuildIdProvider {
    fn handle(&mut self, _payload: &[u8]) -> ProviderOutcome {
        immediate(tela_bridge::encode_app_build_id_response(
            &tela_bridge::AppBuildIdInfo { build_id: self.0 },
        ))
    }
}

/// Bundle version from build constants.
pub struct BuildBundleVersionProvider(pub Version);

impl Provider for BuildBundleVersionProvider {
    fn handle(&mut self, _payload: &[u8]) -> ProviderOutcome {
        immediate(tela_bridge::encode_bundle_version_response(
            &tela_bridge::BundleVersionInfo { version: self.0 },
        ))
    }
}

/// Bundle build id from build constants.
pub struct BuildBundleBuildIdProvider(pub u32);

impl Provider for BuildBundleBuildIdProvider {
    fn handle(&mut self, _payload: &[u8]) -> ProviderOutcome {
        immediate(tela_bridge::encode_bundle_build_id_response(
            &tela_bridge::BundleBuildIdInfo { build_id: self.0 },
        ))
    }
}

/// Static config table (host-injected JSON text per key).
pub struct StaticConfigProvider(pub Vec<(String, String)>);

impl Provider for StaticConfigProvider {
    fn handle(&mut self, payload_bytes: &[u8]) -> ProviderOutcome {
        let Ok(key) = tela_bridge::decode_get_config_request(payload_bytes) else {
            return ProviderOutcome::Immediate(Err(BridgeError::UnknownCapability));
        };
        match self.0.iter().find(|(k, _)| *k == key) {
            Some((_, value)) => immediate(tela_bridge::encode_config_response(
                &tela_bridge::ConfigValue {
                    value: value.clone(),
                },
            )),
            None => ProviderOutcome::Immediate(Err(BridgeError::KeyNotFound)),
        }
    }
}

/// canIUse static table backed by the list of actually registered capabilities.
///
/// "Registered is implemented": the table contains exactly the capabilities the host registered
/// (plus canIUse itself and the common bridges), so a query can never report a capability that
/// has no provider.
pub struct TableCanIUseProvider {
    entries: Vec<CapabilityEntry>,
}

impl TableCanIUseProvider {
    fn new(implemented: &[CapabilityId]) -> Self {
        let mut entries: Vec<CapabilityEntry> = implemented
            .iter()
            .map(|capability| CapabilityEntry {
                capability: capability.clone(),
                hit_version: Version::new(1, 0, 0),
            })
            .collect();
        entries.push(CapabilityEntry {
            capability: tela_bridge::capabilities::can_i_use(),
            hit_version: Version::new(1, 0, 0),
        });
        entries.sort_by_key(|entry| entry.capability.to_string());
        Self { entries }
    }
}

impl Provider for TableCanIUseProvider {
    fn handle(&mut self, payload_bytes: &[u8]) -> ProviderOutcome {
        let Ok(target) = tela_bridge::decode_can_i_use_request(payload_bytes) else {
            return ProviderOutcome::Immediate(Err(BridgeError::UnknownCapability));
        };
        let hit = self
            .entries
            .iter()
            .find(|entry| entry.capability == target)
            .map(|entry| entry.hit_version);
        let Some(hit) = hit else {
            return ProviderOutcome::Immediate(Err(BridgeError::UnknownCapability));
        };
        immediate(tela_bridge::encode_can_i_use_response(
            &tela_bridge::CanIUseInfo { hit_version: hit },
        ))
    }
}

/// Registers the common desktop bridges: canIUse table, the five build-constant version bridges,
/// and the static config provider.
///
/// `implemented` must list every capability the shell registers (common + platform), so the
/// canIUse table reflects exactly the registered set.
pub fn register_common_providers(
    dispatcher: &mut tela_bridge::BridgeDispatcher,
    build: &BuildConstants,
    config: Vec<(String, String)>,
    implemented: &[CapabilityId],
) {
    dispatcher.register(
        tela_bridge::capabilities::can_i_use(),
        TableCanIUseProvider::new(implemented),
    );
    dispatcher.register(
        tela_bridge::capabilities::get_app_name(),
        BuildAppNameProvider(build.app_name.clone()),
    );
    dispatcher.register(
        tela_bridge::capabilities::get_app_version(),
        BuildAppVersionProvider(build.app_version),
    );
    dispatcher.register(
        tela_bridge::capabilities::get_app_build_id(),
        BuildAppBuildIdProvider(build.app_build_id),
    );
    dispatcher.register(
        tela_bridge::capabilities::get_bundle_version(),
        BuildBundleVersionProvider(build.bundle_version),
    );
    dispatcher.register(
        tela_bridge::capabilities::get_bundle_build_id(),
        BuildBundleBuildIdProvider(build.bundle_build_id),
    );
    dispatcher.register(
        tela_bridge::capabilities::get_config(),
        StaticConfigProvider(config),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use tela_bridge::{BridgeDispatcher, BridgeRequest, BridgeResult, capabilities};

    fn test_dispatcher() -> BridgeDispatcher {
        let mut dispatcher = BridgeDispatcher::new();
        let implemented = [
            capabilities::get_app_name(),
            capabilities::get_time_stamp(),
            capabilities::get_coordinates(),
        ];
        register_common_providers(
            &mut dispatcher,
            &BuildConstants::default(),
            vec![("app.theme".to_owned(), "\"dark\"".to_owned())],
            &implemented,
        );
        dispatcher
    }

    fn handle(dispatcher: &mut BridgeDispatcher, request: BridgeRequest) -> BridgeResult {
        match dispatcher.handle(request).expect("immediate") {
            tela_bridge::BridgeEvent::Response { result, .. } => result,
        }
    }

    #[test]
    fn build_constant_bridges_report_injected_values() {
        let mut dispatcher = test_dispatcher();
        let result = handle(
            &mut dispatcher,
            BridgeRequest::new(
                1,
                tela_bridge::VersionPolicy::Latest,
                capabilities::get_app_name(),
            ),
        );
        let payload_bytes = match result {
            BridgeResult::Ok(bytes) => bytes,
            other => panic!("unexpected {other:?}"),
        };
        let info = tela_bridge::decode_app_name_response(&payload_bytes).expect("decode");
        assert_eq!(info.name, "Tela 桌面");
    }

    #[test]
    fn can_i_use_table_matches_registered_set() {
        let mut dispatcher = test_dispatcher();
        let result = handle(
            &mut dispatcher,
            BridgeRequest::with_payload(
                2,
                tela_bridge::VersionPolicy::Latest,
                capabilities::can_i_use(),
                tela_bridge::encode_can_i_use_request(&capabilities::get_coordinates()),
            ),
        );
        let BridgeResult::Ok(bytes) = result else {
            panic!("registered capability must be reported");
        };
        let info = tela_bridge::decode_can_i_use_response(&bytes).expect("decode");
        assert_eq!(info.hit_version, Version::new(1, 0, 0));

        let unknown = handle(
            &mut dispatcher,
            BridgeRequest::with_payload(
                3,
                tela_bridge::VersionPolicy::Latest,
                capabilities::can_i_use(),
                tela_bridge::encode_can_i_use_request(&capabilities::get_battery_level()),
            ),
        );
        assert_eq!(
            unknown,
            BridgeResult::Err(BridgeError::UnknownCapability),
            "unregistered capability must fail"
        );
    }

    #[test]
    fn config_hit_and_miss() {
        let mut dispatcher = test_dispatcher();
        let hit = handle(
            &mut dispatcher,
            BridgeRequest::with_payload(
                4,
                tela_bridge::VersionPolicy::Latest,
                capabilities::get_config(),
                tela_bridge::encode_get_config_request("app.theme"),
            ),
        );
        let BridgeResult::Ok(bytes) = hit else {
            panic!("configured key must hit");
        };
        assert_eq!(
            tela_bridge::decode_config_response(&bytes)
                .expect("decode")
                .value,
            "\"dark\""
        );
        let miss = handle(
            &mut dispatcher,
            BridgeRequest::with_payload(
                5,
                tela_bridge::VersionPolicy::Latest,
                capabilities::get_config(),
                tela_bridge::encode_get_config_request("no.such"),
            ),
        );
        assert_eq!(miss, BridgeResult::Err(BridgeError::KeyNotFound));
    }
}
