//! Desktop shell bridge loop driver: drain guest bridge requests, dispatch to providers, and
//! deliver responses. Platform-neutral; each Target supplies its own `BridgeDispatcher` with the
//! providers it implements.

/// Platform-neutral desktop bridge providers (build constants, canIUse table, config).
pub mod common;

use tela_bridge::{BridgeDispatcher, BridgeEvent, encode_event};
use tela_guest_runtime::GuestRuntime;

/// Processes one guest bridge queue round: reads the queued requests (if the guest exposes the
/// bridge ABI), dispatches each through `dispatcher`, and delivers the immediate responses.
///
/// Deferred (async) providers keep their request pending; the shell delivers their completion
/// later via [`BridgeDispatcher::complete`] (e.g. from a platform callback posted back to the
/// UI thread), then calls [`deliver_event`].
///
/// Guests without the bridge ABI are transparently skipped (queue reads return empty).
pub fn process_bridge_requests(
    runtime: &mut GuestRuntime,
    dispatcher: &mut BridgeDispatcher,
) -> Result<(), String> {
    if !runtime.bridge_available() {
        return Ok(());
    }
    let requests = runtime
        .bridge_drain_requests()
        .map_err(|error| error.to_string())?;
    for request in requests {
        if let Some(event) = dispatcher.handle(request) {
            deliver_event(runtime, &event)?;
        }
    }
    Ok(())
}

/// Encodes and delivers one bridge event to the guest.
pub fn deliver_event(runtime: &mut GuestRuntime, event: &BridgeEvent) -> Result<(), String> {
    let packet = encode_event(event).map_err(|error| error.to_string())?;
    runtime
        .bridge_deliver(&packet)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tela_bridge::{BridgeError, BridgeResult, Provider, ProviderOutcome, capabilities};

    struct NameProvider(&'static str);

    impl Provider for NameProvider {
        fn handle(&mut self, _payload: &[u8]) -> ProviderOutcome {
            ProviderOutcome::Immediate(Ok(tela_bridge::encode_app_name_response(
                &tela_bridge::AppNameInfo {
                    name: self.0.to_owned(),
                },
            )))
        }
    }

    #[test]
    fn dispatcher_gate_and_payload_helpers_compile_together() {
        // 桌面壳注册示例：构造 dispatcher 并注册 provider。
        let mut dispatcher = BridgeDispatcher::new()
            .with_registered(capabilities::get_app_name(), NameProvider("桌面"));
        let request = tela_bridge::request_latest(1, capabilities::get_app_name());
        let event = dispatcher.handle(request).expect("immediate");
        let BridgeEvent::Response { request_id, result } = event;
        assert_eq!(request_id, 1);
        assert!(matches!(result, BridgeResult::Ok(_)));
        assert!(
            dispatcher
                .handle(tela_bridge::request_latest(
                    2,
                    capabilities::get_coordinates()
                ))
                .is_some()
        );
    }

    #[test]
    fn unknown_capability_fails_rather_than_hanging() {
        let mut dispatcher = BridgeDispatcher::new();
        let event = dispatcher
            .handle(tela_bridge::request_latest(
                1,
                capabilities::get_battery_level(),
            ))
            .expect("immediate");
        assert!(matches!(
            event,
            BridgeEvent::Response {
                result: BridgeResult::Err(BridgeError::UnknownCapability),
                ..
            }
        ));
    }
}
