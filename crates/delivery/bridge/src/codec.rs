//! Versioned wire codec for bridge packets (postcard + `TLBR` magic).

use std::fmt;

use crate::{BridgeEvent, BridgeRequest};

const PACKET_MAGIC: [u8; 4] = *b"TLBR";
const PACKET_VERSION: u16 = 1;
const HEADER_LEN: usize = PACKET_MAGIC.len() + std::mem::size_of::<u16>();

/// A bridge packet cannot be transported or decoded by the current contract version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BridgeCodecError {
    /// Packet magic does not identify a Tela bridge packet.
    InvalidMagic,
    /// The packet version is newer or older than this contract understands.
    UnsupportedVersion(u16),
    /// The packet payload could not be encoded.
    Encode(String),
    /// The packet payload could not be decoded.
    Decode(String),
    /// A valid payload was followed by unexpected bytes.
    TrailingBytes,
}

impl fmt::Display for BridgeCodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMagic => f.write_str("invalid Tela bridge packet magic"),
            Self::UnsupportedVersion(version) => {
                write!(f, "unsupported Tela bridge packet version {version}")
            }
            Self::Encode(error) => write!(f, "could not encode Tela bridge packet: {error}"),
            Self::Decode(error) => write!(f, "could not decode Tela bridge packet: {error}"),
            Self::TrailingBytes => f.write_str("Tela bridge packet contains trailing bytes"),
        }
    }
}

impl std::error::Error for BridgeCodecError {}

/// Encodes a bridge request with the `TLBR` magic/version header.
pub fn encode_request(request: &BridgeRequest) -> Result<Vec<u8>, BridgeCodecError> {
    encode_packet(request)
}

/// Decodes a bridge request after validating the packet header.
pub fn decode_request(bytes: &[u8]) -> Result<BridgeRequest, BridgeCodecError> {
    decode_packet(bytes)
}

/// Decodes a sequence of concatenated request packets (the drained guest queue).
///
/// The queue is a stream of self-delimiting packets (each carries its own magic/version header);
/// the stream ends exactly when the bytes are exhausted.
pub fn decode_request_stream(bytes: &[u8]) -> Result<Vec<BridgeRequest>, BridgeCodecError> {
    let mut requests = Vec::new();
    let mut offset = 0;
    while offset < bytes.len() {
        let remaining = &bytes[offset..];
        if remaining.len() < HEADER_LEN || remaining[..PACKET_MAGIC.len()] != PACKET_MAGIC {
            return Err(BridgeCodecError::InvalidMagic);
        }
        let version = u16::from_le_bytes([remaining[4], remaining[5]]);
        if version != PACKET_VERSION {
            return Err(BridgeCodecError::UnsupportedVersion(version));
        }
        let (request, rest): (BridgeRequest, &[u8]) =
            postcard::take_from_bytes(&remaining[HEADER_LEN..])
                .map_err(|error| BridgeCodecError::Decode(error.to_string()))?;
        let packet_len = remaining.len() - rest.len();
        offset += packet_len;
        requests.push(request);
    }
    Ok(requests)
}

/// Encodes a bridge event with the `TLBR` magic/version header.
pub fn encode_event(event: &BridgeEvent) -> Result<Vec<u8>, BridgeCodecError> {
    encode_packet(event)
}

/// Decodes a bridge event after validating the packet header.
pub fn decode_event(bytes: &[u8]) -> Result<BridgeEvent, BridgeCodecError> {
    decode_packet(bytes)
}

fn encode_packet<T: serde::Serialize>(payload: &T) -> Result<Vec<u8>, BridgeCodecError> {
    let body = postcard::to_allocvec(payload)
        .map_err(|error| BridgeCodecError::Encode(error.to_string()))?;
    let mut bytes = Vec::with_capacity(HEADER_LEN + body.len());
    bytes.extend_from_slice(&PACKET_MAGIC);
    bytes.extend_from_slice(&PACKET_VERSION.to_le_bytes());
    bytes.extend_from_slice(&body);
    Ok(bytes)
}

fn decode_packet<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, BridgeCodecError> {
    if bytes.len() < HEADER_LEN || bytes[..PACKET_MAGIC.len()] != PACKET_MAGIC {
        return Err(BridgeCodecError::InvalidMagic);
    }
    let version = u16::from_le_bytes([bytes[4], bytes[5]]);
    if version != PACKET_VERSION {
        return Err(BridgeCodecError::UnsupportedVersion(version));
    }
    let (payload, remaining): (T, &[u8]) = postcard::take_from_bytes(&bytes[HEADER_LEN..])
        .map_err(|error| BridgeCodecError::Decode(error.to_string()))?;
    if !remaining.is_empty() {
        return Err(BridgeCodecError::TrailingBytes);
    }
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BridgeError, BridgeResult, VersionPolicy, capabilities, payload};
    use tela_utils::Version;

    fn sample_request() -> BridgeRequest {
        BridgeRequest {
            request_id: 7,
            version: VersionPolicy::Range {
                lower: Some(Version::new(1, 0, 0)),
                upper: None,
            },
            capability: capabilities::get_battery_level(),
            payload: vec![1, 2, 3],
        }
    }

    #[test]
    fn request_round_trips() {
        let request = sample_request();
        let decoded = decode_request(&encode_request(&request).expect("encode")).expect("decode");
        assert_eq!(decoded, request);
    }

    #[test]
    fn event_round_trips() {
        let event = BridgeEvent::Response {
            request_id: 7,
            result: BridgeResult::Err(BridgeError::KeyNotFound),
        };
        let decoded = decode_event(&encode_event(&event).expect("encode")).expect("decode");
        assert_eq!(decoded, event);
    }

    #[test]
    fn rejects_invalid_or_newer_packets() {
        assert_eq!(
            decode_request(b"bad").unwrap_err(),
            BridgeCodecError::InvalidMagic
        );
        let mut bytes = encode_request(&sample_request()).expect("encode");
        bytes[4..6].copy_from_slice(&2_u16.to_le_bytes());
        assert_eq!(
            decode_request(&bytes).unwrap_err(),
            BridgeCodecError::UnsupportedVersion(2)
        );
    }

    #[test]
    fn rejects_trailing_bytes() {
        let mut bytes = encode_request(&sample_request()).expect("encode");
        bytes.push(0);
        assert_eq!(
            decode_request(&bytes).unwrap_err(),
            BridgeCodecError::TrailingBytes
        );
    }

    #[test]
    fn named_scope_request_round_trips() {
        let request = BridgeRequest {
            request_id: 3,
            version: VersionPolicy::Latest,
            capability: crate::CapabilityId::named("shop", "cart", "getCount"),
            payload: payload::encode_can_i_use_request(&capabilities::get_config()),
        };
        let decoded = decode_request(&encode_request(&request).expect("encode")).expect("decode");
        assert_eq!(decoded, request);
        assert_eq!(request.capability.to_string(), "shop.cart.getCount");
    }
}
