//! fixtures 是协议形状的 canonical 样例：任何一侧改动类型，都必须保证这里的
//! 反序列化 → 再序列化 → 深比较仍然通过。

use std::path::Path;
use std::str::FromStr;

use tela_cc_protocol::{
    DownlinkMessage, Event, SyncResponse, UplinkMessage, decode_frame, encode_frame,
};

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read fixture {}: {error}", path.display()))
}

fn roundtrip<T>(name: &str)
where
    T: serde::de::DeserializeOwned + PartialEq + std::fmt::Debug + serde::Serialize,
{
    let source = fixture(name);
    let parsed: T = serde_json::from_str(&source)
        .unwrap_or_else(|error| panic!("parse fixture {name}: {error}"));
    let serialized =
        serde_json::to_string(&parsed).unwrap_or_else(|error| panic!("serialize {name}: {error}"));
    let before = serde_json::Value::from_str(&source).expect("fixture JSON");
    let after = serde_json::Value::from_str(&serialized).expect("roundtrip JSON");
    assert_eq!(before, after, "fixture {name} must roundtrip losslessly");
}

#[test]
fn event_fixtures_roundtrip() {
    roundtrip::<Event>("event_session_created.json");
    roundtrip::<Event>("event_turn_started.json");
    roundtrip::<Event>("event_assistant_text.json");
    roundtrip::<Event>("event_tool_use.json");
    roundtrip::<Event>("event_tool_result.json");
    roundtrip::<Event>("event_turn_result.json");
    roundtrip::<Event>("event_permission_requested.json");
    roundtrip::<Event>("event_permission_resolved.json");
}

#[test]
fn composite_fixtures_roundtrip() {
    roundtrip::<SyncResponse>("sync_response.json");
    roundtrip::<UplinkMessage>("uplink_hello.json");
    roundtrip::<DownlinkMessage>("downlink_run_turn.json");
}

#[test]
fn fixtures_travel_through_the_agent_frame_codec() {
    // 帧 = 4 字节 LE 长度 + JSON 载荷；解码器消费载荷部分。
    fn payload(frame: &[u8]) -> &[u8] {
        &frame[4..]
    }

    let hello: UplinkMessage = serde_json::from_str(&fixture("uplink_hello.json")).expect("parse");
    let frame = encode_frame(&hello).expect("encode frame");
    let decoded: UplinkMessage = decode_frame(payload(&frame)).expect("decode frame");
    assert_eq!(decoded, hello);

    let run: DownlinkMessage =
        serde_json::from_str(&fixture("downlink_run_turn.json")).expect("parse");
    let frame = encode_frame(&run).expect("encode frame");
    let decoded: DownlinkMessage = decode_frame(payload(&frame)).expect("decode frame");
    assert_eq!(decoded, run);
}
