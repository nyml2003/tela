//! Guest-side bridge facade: request queue, correlated pending callbacks, and a unified
//! capability channel.
//!
//! Every capability is addressed identically: `CapabilityId` + payload bytes. The facade
//! provides typed convenience methods for the `std` bridges (using the payload helpers) plus a
//! generic `request` for any capability (target-specific or host business).

use std::collections::HashMap;

use crate::{BridgeEvent, BridgeRequest, BridgeResult, CapabilityId, VersionPolicy, capabilities};

/// A pending callback awaiting its correlated response.
type Callback = Box<dyn FnOnce(BridgeResult)>;

/// Guest-side bridge facade.
///
/// Each request appends its encoded envelope to the guest request queue (the host drains it
/// after every dispatch) and registers a callback keyed by a monotonically increasing
/// `request_id`. Responses may arrive on any frame.
///
/// The guest owns frame publication: callbacks update application state and the guest decides
/// when to publish a new frame.
pub struct GuestBridge {
    /// Encoded envelopes awaiting the host's next queue drain.
    queue: Vec<u8>,
    /// Correlated pending callbacks.
    pending: HashMap<u64, Callback>,
    /// Next request id.
    next_request_id: u64,
}

impl GuestBridge {
    /// Creates an empty bridge facade.
    pub fn new() -> Self {
        Self {
            queue: Vec::new(),
            pending: HashMap::new(),
            next_request_id: 1,
        }
    }

    /// Number of bytes currently queued for the host to drain.
    pub fn request_queue_len(&self) -> u32 {
        self.queue.len() as u32
    }

    /// Takes the encoded request queue and clears it (host drains once per frame loop).
    pub fn take_request_queue(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.queue)
    }

    /// Returns the buffer backing the request queue for direct memory-copy export.
    pub fn request_queue_bytes(&mut self) -> &mut Vec<u8> {
        &mut self.queue
    }

    /// Handles one host-delivered bridge event packet: decodes and dispatches to pending
    /// callbacks by `request_id`.
    pub fn handle_event_packet(&mut self, bytes: &[u8]) -> Result<(), crate::BridgeCodecError> {
        let event = crate::decode_event(bytes)?;
        match event {
            BridgeEvent::Response { request_id, result } => {
                if let Some(callback) = self.pending.remove(&request_id) {
                    callback(result);
                }
            }
        }
        Ok(())
    }

    /// Sends a request to any capability with an explicit version policy and payload.
    pub fn request(
        &mut self,
        capability: CapabilityId,
        version: VersionPolicy,
        payload: Vec<u8>,
        callback: impl FnOnce(BridgeResult) + 'static,
    ) {
        let request_id = self.next_request_id;
        self.next_request_id += 1;
        let request = BridgeRequest {
            request_id,
            version,
            capability,
            payload,
        };
        let packet = crate::encode_request(&request).expect("encode bridge request");
        self.queue.extend_from_slice(&packet);
        self.pending.insert(request_id, Box::new(callback));
    }

    /// Sends a request to any capability with `Latest` policy and an empty payload.
    pub fn request_latest(
        &mut self,
        capability: CapabilityId,
        callback: impl FnOnce(BridgeResult) + 'static,
    ) {
        self.request(capability, VersionPolicy::Latest, Vec::new(), callback);
    }

    // -----------------------------------------------------------------------
    // Typed convenience methods for the `std` bridges.
    // -----------------------------------------------------------------------

    /// `std.base.canIUse`: capability discovery with an explicit version policy.
    pub fn can_i_use(
        &mut self,
        capability: CapabilityId,
        version: VersionPolicy,
        callback: impl FnOnce(BridgeResult) + 'static,
    ) {
        let payload = crate::payload::encode_can_i_use_request(&capability);
        self.request(capabilities::can_i_use(), version, payload, callback);
    }

    /// `std.base.canIUse` sub-request: list all registered capabilities.
    pub fn list_capabilities(
        &mut self,
        version: VersionPolicy,
        callback: impl FnOnce(BridgeResult) + 'static,
    ) {
        self.request(capabilities::can_i_use(), version, Vec::new(), callback);
    }

    /// `std.device.getAppName`.
    pub fn get_app_name(&mut self, callback: impl FnOnce(BridgeResult) + 'static) {
        self.request_latest(capabilities::get_app_name(), callback);
    }

    /// `std.device.getAppVersion`.
    pub fn get_app_version(&mut self, callback: impl FnOnce(BridgeResult) + 'static) {
        self.request_latest(capabilities::get_app_version(), callback);
    }

    /// `std.device.getAppBuildId`.
    pub fn get_app_build_id(&mut self, callback: impl FnOnce(BridgeResult) + 'static) {
        self.request_latest(capabilities::get_app_build_id(), callback);
    }

    /// `std.device.getBundleVersion`.
    pub fn get_bundle_version(&mut self, callback: impl FnOnce(BridgeResult) + 'static) {
        self.request_latest(capabilities::get_bundle_version(), callback);
    }

    /// `std.device.getBundleBuildId`.
    pub fn get_bundle_build_id(&mut self, callback: impl FnOnce(BridgeResult) + 'static) {
        self.request_latest(capabilities::get_bundle_build_id(), callback);
    }

    /// `std.device.getTimeStamp`.
    pub fn get_time_stamp(&mut self, callback: impl FnOnce(BridgeResult) + 'static) {
        self.request_latest(capabilities::get_time_stamp(), callback);
    }

    /// `std.device.getViewportSize`.
    pub fn get_viewport_size(&mut self, callback: impl FnOnce(BridgeResult) + 'static) {
        self.request_latest(capabilities::get_viewport_size(), callback);
    }

    /// `std.device.getViewportDpr`.
    pub fn get_viewport_dpr(&mut self, callback: impl FnOnce(BridgeResult) + 'static) {
        self.request_latest(capabilities::get_viewport_dpr(), callback);
    }

    /// `std.device.getBatteryLevel`.
    pub fn get_battery_level(&mut self, callback: impl FnOnce(BridgeResult) + 'static) {
        self.request_latest(capabilities::get_battery_level(), callback);
    }

    /// `std.device.getBatteryCharging`.
    pub fn get_battery_charging(&mut self, callback: impl FnOnce(BridgeResult) + 'static) {
        self.request_latest(capabilities::get_battery_charging(), callback);
    }

    /// `std.device.getNetworkOnline`.
    pub fn get_network_online(&mut self, callback: impl FnOnce(BridgeResult) + 'static) {
        self.request_latest(capabilities::get_network_online(), callback);
    }

    /// `std.device.getNetworkKind`.
    pub fn get_network_kind(&mut self, callback: impl FnOnce(BridgeResult) + 'static) {
        self.request_latest(capabilities::get_network_kind(), callback);
    }

    /// `std.position.getCoordinates`.
    pub fn get_coordinates(&mut self, callback: impl FnOnce(BridgeResult) + 'static) {
        self.request_latest(capabilities::get_coordinates(), callback);
    }

    /// `std.config.getConfig`.
    pub fn get_config(
        &mut self,
        key: impl Into<String>,
        callback: impl FnOnce(BridgeResult) + 'static,
    ) {
        let payload = crate::payload::encode_get_config_request(&key.into());
        self.request(
            capabilities::get_config(),
            VersionPolicy::Latest,
            payload,
            callback,
        );
    }
}

impl Default for GuestBridge {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tela_utils::Version;

    use crate::payload;
    use crate::{BridgeResult, model::AppNameInfo};

    fn drain_and_respond(bridge: &mut GuestBridge, with: impl Fn(BridgeRequest) -> BridgeResult) {
        let queue = bridge.take_request_queue();
        let requests = crate::decode_request_stream(&queue).expect("decode queued requests");
        for request in requests {
            let event = BridgeEvent::Response {
                request_id: request.request_id,
                result: with(request),
            };
            bridge
                .handle_event_packet(&crate::encode_event(&event).expect("encode"))
                .expect("handle");
        }
    }

    #[test]
    fn request_queue_is_appended_and_callbacks_fire_in_order() {
        use std::cell::RefCell;
        use std::rc::Rc;

        let mut bridge = GuestBridge::new();
        let order: Rc<RefCell<Vec<(&'static str, BridgeResult)>>> =
            Rc::new(RefCell::new(Vec::new()));
        bridge.get_app_name({
            let order = Rc::clone(&order);
            move |result| {
                order.borrow_mut().push(("name", result));
            }
        });
        bridge.get_battery_level({
            let order = Rc::clone(&order);
            move |result| {
                order.borrow_mut().push(("battery", result));
            }
        });
        assert!(bridge.request_queue_len() > 0);

        drain_and_respond(&mut bridge, |request| {
            match request.capability.to_string().as_str() {
                "std.device.getAppName" => {
                    BridgeResult::ok(payload::encode_app_name_response(&AppNameInfo {
                        name: "demo".to_owned(),
                    }))
                }
                "std.device.getBatteryLevel" => BridgeResult::ok(
                    payload::encode_battery_level_response(&crate::BatteryLevelInfo { level: 0.5 }),
                ),
                other => panic!("unexpected request {other}"),
            }
        });

        let order = order.borrow();
        assert_eq!(order.len(), 2);
        assert_eq!(order[0].0, "name");
        assert_eq!(order[1].0, "battery");
        assert_eq!(bridge.request_queue_len(), 0);
        assert!(bridge.pending.is_empty());
    }

    #[test]
    fn request_ids_are_monotonic_and_distinct() {
        let mut bridge = GuestBridge::new();
        bridge.get_config("k", |_| {});
        bridge.get_coordinates(|_| {});
        let queue = bridge.take_request_queue();
        let requests = crate::decode_request_stream(&queue).expect("decode");
        let first = &requests[0];
        let second = &requests[1];
        assert_eq!(first.request_id, 1);
        assert_eq!(second.request_id, 2);
        assert_ne!(first.request_id, second.request_id);
    }

    #[test]
    fn can_i_use_carries_policy_and_capability_in_payload() {
        let mut bridge = GuestBridge::new();
        bridge.can_i_use(
            capabilities::get_network_online(),
            VersionPolicy::Exact(Version::new(1, 0, 0)),
            |_| {},
        );
        let queue = bridge.take_request_queue();
        let request = crate::decode_request(&queue).expect("request");
        assert_eq!(request.capability, capabilities::can_i_use());
        assert_eq!(request.version, VersionPolicy::Exact(Version::new(1, 0, 0)));
        let target = payload::decode_can_i_use_request(&request.payload).expect("payload");
        assert_eq!(target, capabilities::get_network_online());
    }

    #[test]
    fn named_capability_requests_carry_raw_payload() {
        let mut bridge = GuestBridge::new();
        let shop_cart = CapabilityId::named("shop", "cart", "getCount");
        bridge.request(
            shop_cart.clone(),
            VersionPolicy::Latest,
            vec![1, 2, 3],
            |_| {},
        );
        let queue = bridge.take_request_queue();
        let request = crate::decode_request(&queue).expect("request");
        assert_eq!(request.capability, shop_cart);
        assert_eq!(request.payload, vec![1, 2, 3]);
    }
}
