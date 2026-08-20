//! Host-side bridge dispatcher: a capability registry (unified for `std`, target-specific, and
//! host business bridges) with version gating and async completion.

use std::collections::{HashMap, HashSet};

use crate::{BridgeError, BridgeEvent, BridgeRequest, BridgeResult, CapabilityId, VersionPolicy};
use tela_utils::Version;

/// Outcome of one provider invocation.
#[derive(Clone, Debug, PartialEq)]
pub enum ProviderOutcome {
    /// The result is ready and should be echoed in the same queue-processing round.
    Immediate(Result<Vec<u8>, BridgeError>),
    /// The provider defers (e.g. platform permission flow); the host completes the request
    /// later via [`BridgeDispatcher::complete`].
    Pending,
}

/// One provider serving a capability through the unified byte channel.
///
/// Every capability — `std` published contracts, target-specific bridges, and host business
/// bridges — is registered in the same registry and executed identically.
pub trait Provider {
    /// Implementation version of this capability.
    fn version(&self) -> Version {
        Version::new(1, 0, 0)
    }
    /// Handles one request payload; may defer with [`ProviderOutcome::Pending`].
    fn handle(&mut self, payload: &[u8]) -> ProviderOutcome;
}

/// Dispatches decoded bridge requests to the registered capability providers.
///
/// Unregistered capabilities fail with [`BridgeError::UnknownCapability`] instead of hanging.
pub struct BridgeDispatcher {
    /// Unified capability registry (std + named scopes share one table).
    providers: HashMap<CapabilityId, Box<dyn Provider>>,
    /// Request ids currently deferred by async providers.
    pending: HashSet<u64>,
}

impl BridgeDispatcher {
    /// Creates a dispatcher with no providers registered (every request fails with
    /// `UnknownCapability` until providers are attached).
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
            pending: HashSet::new(),
        }
    }

    /// Registers a provider for a capability (idempotent overwrite).
    pub fn register(&mut self, capability: CapabilityId, provider: impl Provider + 'static) {
        self.providers.insert(capability, Box::new(provider));
    }

    /// Builds a dispatcher and registers one provider (builder style).
    pub fn with_registered(
        mut self,
        capability: CapabilityId,
        provider: impl Provider + 'static,
    ) -> Self {
        self.register(capability, provider);
        self
    }

    /// Returns the registered capability entries with their implementation versions.
    pub fn entries(&self) -> Vec<crate::CapabilityEntry> {
        self.providers
            .iter()
            .map(|(capability, provider)| crate::CapabilityEntry {
                capability: capability.clone(),
                hit_version: provider.version(),
            })
            .collect()
    }

    /// Returns whether a capability is registered.
    pub fn is_registered(&self, capability: &CapabilityId) -> bool {
        self.providers.contains_key(capability)
    }

    /// Processes one decoded request.
    ///
    /// Returns `None` when the provider deferred (async completion via [`Self::complete`]);
    /// otherwise returns the response event to encode and deliver.
    pub fn handle(&mut self, request: BridgeRequest) -> Option<BridgeEvent> {
        let BridgeRequest {
            request_id,
            version,
            capability,
            payload,
        } = request;
        match self.dispatch(&capability, version, &payload) {
            DispatchResult::Immediate(result) => Some(BridgeEvent::Response { request_id, result }),
            DispatchResult::Pending => {
                self.pending.insert(request_id);
                None
            }
        }
    }

    /// Completes a deferred (async) request. Returns `None` if the id is unknown.
    pub fn complete(&mut self, request_id: u64, result: BridgeResult) -> Option<BridgeEvent> {
        if self.pending.remove(&request_id) {
            Some(BridgeEvent::Response { request_id, result })
        } else {
            None
        }
    }

    /// Pending (deferred) request ids, useful for diagnostics and tests.
    pub fn pending_ids(&self) -> &HashSet<u64> {
        &self.pending
    }

    fn dispatch(
        &mut self,
        capability: &CapabilityId,
        version: VersionPolicy,
        payload: &[u8],
    ) -> DispatchResult {
        let Some(provider) = self.providers.get_mut(capability) else {
            return DispatchResult::Immediate(BridgeResult::err(BridgeError::UnknownCapability));
        };
        let available = provider.version();
        if !version.matches(available) {
            return DispatchResult::Immediate(BridgeResult::err(BridgeError::VersionMismatch {
                policy: version,
                available,
            }));
        }
        match provider.handle(payload) {
            ProviderOutcome::Immediate(result) => DispatchResult::Immediate(match result {
                Ok(bytes) => BridgeResult::ok(bytes),
                Err(error) => BridgeResult::err(error),
            }),
            ProviderOutcome::Pending => DispatchResult::Pending,
        }
    }
}

impl Default for BridgeDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

enum DispatchResult {
    Immediate(BridgeResult),
    Pending,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::payload;
    use crate::{CapabilityEntry, model::AppNameInfo};

    struct StaticName(&'static str);

    impl Provider for StaticName {
        fn handle(&mut self, _payload: &[u8]) -> ProviderOutcome {
            ProviderOutcome::Immediate(Ok(payload::encode_app_name_response(&AppNameInfo {
                name: self.0.to_owned(),
            })))
        }
    }

    struct DeferredCoordinates;

    impl Provider for DeferredCoordinates {
        fn handle(&mut self, _payload: &[u8]) -> ProviderOutcome {
            ProviderOutcome::Pending
        }
    }

    struct StaticCanIUse;

    impl Provider for StaticCanIUse {
        fn handle(&mut self, payload: &[u8]) -> ProviderOutcome {
            match payload::decode_can_i_use_request(payload) {
                Ok(capability) if capability == crate::capabilities::get_app_name() => {
                    ProviderOutcome::Immediate(Ok(payload::encode_can_i_use_response(
                        &crate::CanIUseInfo {
                            hit_version: Version::new(1, 0, 0),
                        },
                    )))
                }
                _ => ProviderOutcome::Immediate(Err(BridgeError::UnknownCapability)),
            }
        }
    }

    struct StaticCount(u8);

    impl Provider for StaticCount {
        fn handle(&mut self, _payload: &[u8]) -> ProviderOutcome {
            ProviderOutcome::Immediate(Ok(vec![self.0]))
        }
    }

    fn request(request_id: u64, capability: CapabilityId) -> BridgeRequest {
        BridgeRequest {
            request_id,
            version: VersionPolicy::Latest,
            capability,
            payload: Vec::new(),
        }
    }

    #[test]
    fn unregistered_capability_fails_unknown() {
        let mut dispatcher = BridgeDispatcher::new();
        let event = dispatcher
            .handle(request(1, crate::capabilities::get_battery_level()))
            .expect("immediate");
        assert_eq!(
            event,
            BridgeEvent::Response {
                request_id: 1,
                result: BridgeResult::err(BridgeError::UnknownCapability)
            }
        );
    }

    #[test]
    fn registered_provider_echoes_immediately() {
        let mut dispatcher = BridgeDispatcher::new()
            .with_registered(crate::capabilities::get_app_name(), StaticName("demo"));
        let event = dispatcher
            .handle(request(2, crate::capabilities::get_app_name()))
            .expect("immediate");
        assert_eq!(
            event,
            BridgeEvent::Response {
                request_id: 2,
                result: BridgeResult::ok(payload::encode_app_name_response(&AppNameInfo {
                    name: "demo".to_owned()
                }))
            }
        );
    }

    #[test]
    fn version_gate_rejects_mismatch() {
        let mut dispatcher = BridgeDispatcher::new()
            .with_registered(crate::capabilities::get_app_name(), StaticName("demo"));
        let event = dispatcher
            .handle(BridgeRequest {
                request_id: 3,
                version: VersionPolicy::Exact(Version::new(9, 0, 0)),
                capability: crate::capabilities::get_app_name(),
                payload: Vec::new(),
            })
            .expect("immediate");
        assert_eq!(
            event,
            BridgeEvent::Response {
                request_id: 3,
                result: BridgeResult::err(BridgeError::VersionMismatch {
                    policy: VersionPolicy::Exact(Version::new(9, 0, 0)),
                    available: Version::new(1, 0, 0),
                })
            }
        );
    }

    #[test]
    fn deferred_provider_completes_via_complete() {
        let mut dispatcher = BridgeDispatcher::new()
            .with_registered(crate::capabilities::get_coordinates(), DeferredCoordinates);
        assert!(
            dispatcher
                .handle(request(4, crate::capabilities::get_coordinates()))
                .is_none()
        );
        assert!(dispatcher.pending_ids().contains(&4));

        let coords = crate::Coordinates {
            latitude: 31.23,
            longitude: 121.47,
            accuracy_meters: 5.0,
            timestamp_millis: 1_700_000_000_000,
            datum: crate::Datum::Wgs84,
        };
        let event = dispatcher
            .complete(
                4,
                BridgeResult::ok(payload::encode_coordinates_response(&coords)),
            )
            .expect("completed");
        assert_eq!(
            event,
            BridgeEvent::Response {
                request_id: 4,
                result: BridgeResult::ok(payload::encode_coordinates_response(&coords))
            }
        );
        assert!(!dispatcher.pending_ids().contains(&4));
        assert!(
            dispatcher
                .complete(4, BridgeResult::err(BridgeError::Timeout))
                .is_none()
        );
    }

    #[test]
    fn named_scope_business_bridge_works_through_registry() {
        let shop_cart = CapabilityId::named("shop", "cart", "getCount");
        let mut dispatcher =
            BridgeDispatcher::new().with_registered(shop_cart.clone(), StaticCount(3));
        let event = dispatcher.handle(request(5, shop_cart)).expect("immediate");
        match event {
            BridgeEvent::Response {
                request_id: 5,
                result: BridgeResult::Ok(payload),
            } => assert_eq!(payload, vec![3]),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn can_i_use_and_list_work_through_registry() {
        let mut dispatcher = BridgeDispatcher::new()
            .with_registered(crate::capabilities::can_i_use(), StaticCanIUse)
            .with_registered(crate::capabilities::get_app_name(), StaticName("demo"));
        let hit = dispatcher
            .handle(BridgeRequest {
                request_id: 6,
                version: VersionPolicy::Latest,
                capability: crate::capabilities::can_i_use(),
                payload: payload::encode_can_i_use_request(&crate::capabilities::get_app_name()),
            })
            .expect("immediate");
        assert_eq!(
            hit,
            BridgeEvent::Response {
                request_id: 6,
                result: BridgeResult::ok(payload::encode_can_i_use_response(&crate::CanIUseInfo {
                    hit_version: Version::new(1, 0, 0)
                }))
            }
        );
        let entries = dispatcher.entries();
        assert_eq!(entries.len(), 2);
        assert!(entries.contains(&CapabilityEntry {
            capability: crate::capabilities::get_app_name(),
            hit_version: Version::new(1, 0, 0)
        }));
    }
}
