//! 宿主桥 `net.http.request` 能力的载荷格式（postcard，与 tela-bridge 的 payload 风格一致）。
//!
//! guest 只看到相对 `path` 与原始 `body`；base_url 与 Bearer token 是宿主侧配置，永不进入
//! guest WASM（token 安全边界，见 docs/039）。`status = 0` 表示传输层失败，body 携带短错误
//! 串——不扩 `BridgeError` 枚举，桥契约保持零改动。

use serde::{Deserialize, Serialize, de::DeserializeOwned};

/// 具名能力 scope：`net`。
pub const NET_SCOPE: &str = "net";
/// 具名能力 group：`http`。
pub const NET_GROUP: &str = "http";
/// 具名能力 name：`request`。
pub const NET_NAME: &str = "request";

/// 能力的展示形式（与 tela-bridge `CapabilityId::Display` 一致）。
pub fn net_capability_display() -> String {
    format!("{NET_SCOPE}.{NET_GROUP}.{NET_NAME}")
}

/// 请求方法子集：手机端只需要 GET（轮询）与 POST（发消息/批权限）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetHttpMethod {
    Get,
    Post,
}

/// 一次出站请求；`path` 是相对 base_url 的路径（含 query）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetHttpRequest {
    pub method: NetHttpMethod,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<Vec<u8>>,
}

/// 一次入站响应；`status = 0` 表示传输层失败（body 为短错误串）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetHttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
    /// body 达到 [`crate::MAX_RESPONSE_BODY_BYTES`] 上限被截断；客户端应按 cursor 分页。
    pub truncated: bool,
}

impl NetHttpResponse {
    /// 构造传输层失败响应（host 无法连通、DNS、超时等）。
    pub fn transport_error(message: impl Into<String>) -> Self {
        Self {
            status: 0,
            body: message.into().into_bytes(),
            truncated: false,
        }
    }

    /// 把 body 解析为 JSON 值；传输失败或非 JSON 时报错。
    pub fn json<T: DeserializeOwned>(&self) -> Result<T, NetPayloadError> {
        serde_json::from_slice(&self.body)
            .map_err(|error| NetPayloadError::Decode(format!("response body: {error}")))
    }
}

/// 载荷编解码失败。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetPayloadError {
    Encode(String),
    Decode(String),
}

impl std::fmt::Display for NetPayloadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Encode(message) => write!(formatter, "could not encode net payload: {message}"),
            Self::Decode(message) => write!(formatter, "could not decode net payload: {message}"),
        }
    }
}

impl std::error::Error for NetPayloadError {}

fn encode<T: Serialize>(value: &T) -> Vec<u8> {
    postcard::to_allocvec(value).expect("encode net bridge payload")
}

fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, NetPayloadError> {
    postcard::from_bytes(bytes).map_err(|error| NetPayloadError::Decode(error.to_string()))
}

/// 编码 `net.http.request` 请求载荷。
pub fn encode_net_http_request(request: &NetHttpRequest) -> Vec<u8> {
    encode(request)
}

/// 解码 `net.http.request` 请求载荷。
pub fn decode_net_http_request(bytes: &[u8]) -> Result<NetHttpRequest, NetPayloadError> {
    decode(bytes)
}

/// 编码 `net.http.request` 响应载荷。
pub fn encode_net_http_response(response: &NetHttpResponse) -> Vec<u8> {
    encode(response)
}

/// 解码 `net.http.request` 响应载荷。
pub fn decode_net_http_response(bytes: &[u8]) -> Result<NetHttpResponse, NetPayloadError> {
    decode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_response_roundtrip_through_postcard() {
        let request = NetHttpRequest {
            method: NetHttpMethod::Post,
            path: "/v1/sessions/s1/messages".to_owned(),
            body: Some(br#"{"text":"hi","client_msg_id":"c1"}"#.to_vec()),
        };
        let bytes = encode_net_http_request(&request);
        assert_eq!(decode_net_http_request(&bytes).expect("decode"), request);

        let response = NetHttpResponse {
            status: 200,
            body: br#"{"accepted":true}"#.to_vec(),
            truncated: false,
        };
        let bytes = encode_net_http_response(&response);
        assert_eq!(decode_net_http_response(&bytes).expect("decode"), response);
    }

    #[test]
    fn transport_error_response_decodes_with_status_zero() {
        let response = NetHttpResponse::transport_error("dial tcp: connection refused");
        assert_eq!(response.status, 0);
        let decoded =
            decode_net_http_response(&encode_net_http_response(&response)).expect("decode");
        assert_eq!(decoded, response);
        assert!(decoded.json::<serde_json::Value>().is_err());
    }

    #[test]
    fn capability_display_matches_bridge_convention() {
        assert_eq!(net_capability_display(), "net.http.request");
    }
}
