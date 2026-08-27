//! Android 宿主的桥接线：`net.http.request` 具名能力、canIUse 静态表与回投泵。
//!
//! 分工（docs/039）：guest 排队相对路径请求 → 宿主 drain 后经 [`pump_bridge`] 投给
//! [`RelayNetWorker`] 的网络线程（ureq 阻塞 IO）→ 完成后经 `HostEvent::BridgeResponse`
//! 回到事件循环 → `dispatcher.complete` 产生响应事件 → `bridge_deliver` 回投 guest →
//! 补一次 `AppEvent::Wake` 让 guest 消费回调并发布新帧。
//!
//! `Provider::handle` 拿不到 `request_id`，因此真实网络投递发生在 [`pump_bridge`]（drain
//! 侧天然知道 id）；[`RelayNetProvider`] 只负责让 dispatcher 通过版本门并登记 Pending。

use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use tela_bridge::{
    BridgeDispatcher, BridgeError, BridgeEvent, BridgeResult, CapabilityEntry, CapabilityId,
    Provider, ProviderOutcome, capabilities, encode_event,
};
use tela_cc_protocol::{
    DEFAULT_POLL_INTERVAL_MS, MAX_RESPONSE_BODY_BYTES, NET_GROUP, NET_NAME, NET_SCOPE,
    NetHttpMethod, NetHttpRequest, NetHttpResponse, decode_net_http_request,
    encode_net_http_response,
};
use tela_guest_runtime::GuestRuntime;
use tela_utils::Version;
use winit::event_loop::EventLoopProxy;

use crate::android::HostEvent;

/// `net.http.request` 的能力 id（具名 scope，不进 std）。
pub(crate) fn net_capability() -> CapabilityId {
    CapabilityId::named(NET_SCOPE, NET_GROUP, NET_NAME)
}

/// 宿主侧中继配置；base_url 与 token 永不进入 guest WASM。
#[derive(Clone, Debug)]
pub(crate) struct RelayConfig {
    /// REST 基址，例如 `http://127.0.0.1:8787`。
    pub base_url: String,
    /// Bearer token。
    pub token: String,
}

/// 一条待执行的网络作业（request_id 用于完成后 `dispatcher.complete` 关联）。
struct NetJob {
    request_id: u64,
    method: NetHttpMethod,
    path: String,
    body: Option<Vec<u8>>,
}

/// 阻塞 IO 网络线程的宿主句柄；线程随事件循环退出后 `send_event` 失败自然结束。
pub(crate) struct RelayNetWorker {
    tx: Sender<NetJob>,
}

impl RelayNetWorker {
    /// 启动网络线程；完成后逐条经 `HostEvent::BridgeResponse` 送回事件循环。
    pub(crate) fn start(config: RelayConfig, proxy: EventLoopProxy<HostEvent>) -> Self {
        let (tx, rx) = std::sync::mpsc::channel::<NetJob>();
        thread::spawn(move || {
            while let Ok(job) = rx.recv() {
                let result = relay_http(&config, &job);
                // 事件循环可能已退出；发送失败仅忽略，recv 关闭后线程自然结束。
                let _ = proxy.send_event(HostEvent::BridgeResponse {
                    request_id: job.request_id,
                    result,
                });
            }
        });
        Self { tx }
    }

    /// 投递一条网络作业（drain 侧调用；pump 已通过 dispatcher 登记 Pending）。
    pub(crate) fn submit(&self, request_id: u64, request: NetHttpRequest) {
        let _ = self.tx.send(NetJob {
            request_id,
            method: request.method,
            path: request.path,
            body: request.body,
        });
    }
}

/// 执行一次 REST 往返；传输层失败折叠为 `status = 0` 的成功载荷（docs/039 的错误语义）。
fn relay_http(config: &RelayConfig, job: &NetJob) -> BridgeResult {
    let url = format!("{}{}", config.base_url, job.path);
    let authorization = format!("Bearer {}", config.token);
    // ureq 的 builder 是类型状态的（WithBody/WithoutBody 方法集不同），按方法分流；
    // POST 统一走 send（协议上只有 POST 携带 body，空体发空字节）。
    let send = match job.method {
        NetHttpMethod::Get => ureq::get(&url)
            .header("Authorization", &authorization)
            .call(),
        NetHttpMethod::Post => {
            let request = ureq::post(&url)
                .header("Authorization", &authorization)
                .header("Content-Type", "application/json");
            match job.body.as_deref() {
                Some(body) => request.send(body),
                None => request.send(&[]),
            }
        }
    };
    let response = match send {
        Ok(response) => response,
        Err(error) => {
            return BridgeResult::Ok(encode_net_http_response(&NetHttpResponse::transport_error(
                format!("{url}: {error}"),
            )));
        }
    };
    let status = response.status().as_u16();
    let mut body = match response
        .into_body()
        .into_with_config()
        .limit(u64::try_from(MAX_RESPONSE_BODY_BYTES + 1).expect("response byte limit fits in u64"))
        .read_to_vec()
    {
        Ok(body) => body,
        Err(error) => {
            return BridgeResult::Ok(encode_net_http_response(&NetHttpResponse::transport_error(
                format!("{url}: {error}"),
            )));
        }
    };
    // 多读一字节只为判断截断；超出上限的部分丢弃。
    let truncated = body.len() > MAX_RESPONSE_BODY_BYTES;
    if truncated {
        body.truncate(MAX_RESPONSE_BODY_BYTES);
    }
    BridgeResult::Ok(encode_net_http_response(&NetHttpResponse {
        status,
        body,
        truncated,
    }))
}

/// `net.http.request` 的登记器：真实投递在 [`pump_bridge`]（它持有 request_id）。
pub(crate) struct RelayNetProvider;

impl Provider for RelayNetProvider {
    fn version(&self) -> Version {
        Version::new(1, 0, 0)
    }

    fn handle(&mut self, _payload: &[u8]) -> ProviderOutcome {
        ProviderOutcome::Pending
    }
}

/// canIUse 静态表："注册即实现"，条目恰为本宿主注册的能力集。
pub(crate) struct TableCanIUseProvider {
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
            capability: capabilities::can_i_use(),
            hit_version: Version::new(1, 0, 0),
        });
        entries.sort_by_key(|entry| entry.capability.to_string());
        Self { entries }
    }
}

impl Provider for TableCanIUseProvider {
    fn handle(&mut self, payload: &[u8]) -> ProviderOutcome {
        let Ok(target) = tela_bridge::decode_can_i_use_request(payload) else {
            return ProviderOutcome::Immediate(Err(BridgeError::UnknownCapability));
        };
        let Some(hit) = self
            .entries
            .iter()
            .find(|entry| entry.capability == target)
            .map(|entry| entry.hit_version)
        else {
            return ProviderOutcome::Immediate(Err(BridgeError::UnknownCapability));
        };
        ProviderOutcome::Immediate(Ok(tela_bridge::encode_can_i_use_response(
            &tela_bridge::CanIUseInfo { hit_version: hit },
        )))
    }
}

/// 组装本宿主的桥注册表：canIUse 表 + `net.http.request`。
pub(crate) fn build_dispatcher() -> BridgeDispatcher {
    let net = net_capability();
    let mut dispatcher = BridgeDispatcher::new();
    dispatcher.register(
        capabilities::can_i_use(),
        TableCanIUseProvider::new(&[net.clone()]),
    );
    dispatcher.register(net, RelayNetProvider);
    dispatcher
}

/// 把一条响应事件编码后回投 guest。
fn deliver(runtime: &mut GuestRuntime, event: &BridgeEvent) -> Result<(), String> {
    let packet = encode_event(event).map_err(|error| error.to_string())?;
    runtime
        .bridge_deliver(&packet)
        .map_err(|error| error.to_string())
}

/// 泵一轮桥：drain guest 请求 → 分发 → Immediate 结果就地回投。
///
/// `net.http.request` 的请求在此投给网络线程（request_id 只在这里可见）；其余能力走
/// dispatcher 的同步路径。返回本轮就地回投的事件数，供调用方决定是否补 `Wake`。
pub(crate) fn pump_bridge(
    runtime: &mut GuestRuntime,
    dispatcher: &mut BridgeDispatcher,
    worker: &RelayNetWorker,
) -> Result<u32, String> {
    let requests = runtime
        .bridge_drain_requests()
        .map_err(|error| error.to_string())?;
    let mut delivered = 0u32;
    for request in requests {
        if request.capability == net_capability() {
            let request_id = request.request_id;
            match decode_net_http_request(&request.payload) {
                Ok(net_request) => match dispatcher.handle(request) {
                    // 版本门通过并登记 Pending；真实 IO 由网络线程完成。
                    None => worker.submit(request_id, net_request),
                    Some(event) => {
                        deliver(runtime, &event)?;
                        delivered += 1;
                    }
                },
                Err(error) => {
                    let event = BridgeEvent::Response {
                        request_id,
                        result: BridgeResult::err(BridgeError::UnknownCapability),
                    };
                    let _ = error;
                    deliver(runtime, &event)?;
                    delivered += 1;
                }
            }
        } else {
            match dispatcher.handle(request) {
                Some(event) => {
                    deliver(runtime, &event)?;
                    delivered += 1;
                }
                // 本宿主只有 net 一个异步能力；其余 Pending（当前不存在）无回投路径。
                None => {}
            }
        }
    }
    Ok(delivered)
}

/// 启动轮询心跳线程；返回停止开关。线程每 DEFAULT_POLL_INTERVAL_MS 发一次 `PollTick`。
pub(crate) fn start_poll_thread(
    proxy: EventLoopProxy<HostEvent>,
) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_millis(DEFAULT_POLL_INTERVAL_MS));
            if thread_stop.load(Ordering::Relaxed) {
                break;
            }
            if proxy
                .send_event(HostEvent::PollTick(crate::android::monotonic_ms()))
                .is_err()
            {
                break;
            }
        }
    });
    stop
}

#[cfg(test)]
mod tests {
    use super::*;
    use tela_bridge::{BridgeRequest, VersionPolicy};

    fn net_request(request_id: u64, request: &NetHttpRequest) -> BridgeRequest {
        BridgeRequest {
            request_id,
            version: VersionPolicy::Latest,
            capability: net_capability(),
            payload: tela_cc_protocol::encode_net_http_request(request),
        }
    }

    #[test]
    fn can_i_use_table_reports_exactly_the_registered_set() {
        let mut dispatcher = build_dispatcher();
        let probe = BridgeRequest {
            request_id: 1,
            version: VersionPolicy::Latest,
            capability: capabilities::can_i_use(),
            payload: tela_bridge::encode_can_i_use_request(&net_capability()),
        };
        let Some(BridgeEvent::Response {
            result: BridgeResult::Ok(bytes),
            ..
        }) = dispatcher.handle(probe)
        else {
            panic!("registered net capability must be reported");
        };
        let info = tela_bridge::decode_can_i_use_response(&bytes).expect("decode");
        assert_eq!(info.hit_version, Version::new(1, 0, 0));

        let missing = BridgeRequest {
            request_id: 2,
            version: VersionPolicy::Latest,
            capability: capabilities::can_i_use(),
            payload: tela_bridge::encode_can_i_use_request(&capabilities::get_battery_level()),
        };
        let Some(BridgeEvent::Response { result, .. }) = dispatcher.handle(missing) else {
            panic!("immediate");
        };
        assert_eq!(result, BridgeResult::err(BridgeError::UnknownCapability));
    }

    #[test]
    fn net_provider_defers_and_completes_by_request_id() {
        let mut dispatcher = build_dispatcher();
        let request = net_request(
            7,
            &NetHttpRequest {
                method: NetHttpMethod::Get,
                path: "/v1/sync?since=0".to_owned(),
                body: None,
            },
        );
        // pump 的 dispatcher.handle 路径：版本门通过 → Pending 登记，无就地事件。
        assert!(dispatcher.handle(request).is_none());
        assert!(dispatcher.pending_ids().contains(&7));

        // 网络线程完成后经 complete 关联回 request_id。
        let response = NetHttpResponse {
            status: 200,
            body: b"{}".to_vec(),
            truncated: false,
        };
        let Some(BridgeEvent::Response {
            request_id,
            result: BridgeResult::Ok(bytes),
        }) = dispatcher.complete(7, BridgeResult::Ok(encode_net_http_response(&response)))
        else {
            panic!("pending request must complete");
        };
        assert_eq!(request_id, 7);
        assert_eq!(
            tela_cc_protocol::decode_net_http_response(&bytes).expect("decode"),
            response
        );
        // 重复 complete 不会再产生事件。
        assert!(
            dispatcher
                .complete(7, BridgeResult::err(BridgeError::Timeout))
                .is_none()
        );
    }

    #[test]
    fn transport_failure_is_a_status_zero_success_payload() {
        // relay_http 的错误折叠语义在协议侧单测覆盖；这里只钉住载荷约定。
        let response = NetHttpResponse::transport_error("dial refused");
        let bytes = encode_net_http_response(&response);
        let decoded = tela_cc_protocol::decode_net_http_response(&bytes).expect("decode");
        assert_eq!(decoded.status, 0);
        assert_eq!(decoded.body, b"dial refused".to_vec());
    }
}
