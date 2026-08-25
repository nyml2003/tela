//! Versioned codecs for the platform-neutral application session packets.

use serde::{Serialize, de::DeserializeOwned};
use tela_app_session::{AppEvent, AppStatus};

use crate::FrameCodecError;

const EVENT_MAGIC: [u8; 4] = *b"TLEV";
const STATUS_MAGIC: [u8; 4] = *b"TLSV";
const PACKET_VERSION: u16 = 6;
const HEADER_LEN: usize = EVENT_MAGIC.len() + std::mem::size_of::<u16>();

/// Encodes a host-to-application event with a versioned packet header.
pub fn encode_event(event: &AppEvent) -> Result<Vec<u8>, FrameCodecError> {
    encode_packet(EVENT_MAGIC, event)
}

/// Decodes a host-to-application event after verifying its magic and version.
pub fn decode_event(bytes: &[u8]) -> Result<AppEvent, FrameCodecError> {
    decode_packet(EVENT_MAGIC, bytes)
}

/// Encodes application status for consumption by a platform target.
pub fn encode_status(status: &AppStatus) -> Result<Vec<u8>, FrameCodecError> {
    encode_packet(STATUS_MAGIC, status)
}

/// Decodes application status after verifying its magic and version.
pub fn decode_status(bytes: &[u8]) -> Result<AppStatus, FrameCodecError> {
    decode_packet(STATUS_MAGIC, bytes)
}

fn encode_packet<T: Serialize>(magic: [u8; 4], value: &T) -> Result<Vec<u8>, FrameCodecError> {
    let payload =
        postcard::to_allocvec(value).map_err(|error| FrameCodecError::Encode(error.to_string()))?;
    let mut bytes = Vec::with_capacity(HEADER_LEN + payload.len());
    bytes.extend_from_slice(&magic);
    bytes.extend_from_slice(&PACKET_VERSION.to_le_bytes());
    bytes.extend_from_slice(&payload);
    Ok(bytes)
}

fn decode_packet<T: DeserializeOwned>(magic: [u8; 4], bytes: &[u8]) -> Result<T, FrameCodecError> {
    if bytes.len() < HEADER_LEN || bytes[..magic.len()] != magic {
        return Err(FrameCodecError::InvalidMagic);
    }
    let version = u16::from_le_bytes([bytes[4], bytes[5]]);
    if version != PACKET_VERSION {
        return Err(FrameCodecError::UnsupportedVersion(version));
    }
    let (value, remaining): (T, &[u8]) = postcard::take_from_bytes(&bytes[HEADER_LEN..])
        .map_err(|error| FrameCodecError::Decode(error.to_string()))?;
    if !remaining.is_empty() {
        return Err(FrameCodecError::TrailingBytes);
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tela_app_session::{AppFrameToken, CursorKind};

    #[test]
    fn event_and_status_packets_round_trip() {
        let event = AppEvent::WindowState { maximized: true };
        assert_eq!(decode_event(&encode_event(&event).unwrap()).unwrap(), event);

        let status = AppStatus {
            frame_token: AppFrameToken::new(7),
            cursor: CursorKind::Pointer,
            input_focused: true,
            input_value: "draft".to_owned(),
            animation_active: true,
            next_deadline_ms: Some(42),
        };
        assert_eq!(
            decode_status(&encode_status(&status).unwrap()).unwrap(),
            status
        );
    }
}
