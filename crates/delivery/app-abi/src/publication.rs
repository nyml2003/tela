//! Atomic application publication packet codec.

use serde::{Deserialize, Serialize};
use tela_app_session::{AppEffect, AppFrameToken, AppPublication};
use tela_contract::WindowCommand;

use crate::{FrameCodecError, decode_frame, decode_status, encode_frame, encode_status};

const PUBLICATION_MAGIC: [u8; 4] = *b"TLPB";
const PUBLICATION_VERSION: u16 = 1;
const HEADER_LEN: usize = PUBLICATION_MAGIC.len() + std::mem::size_of::<u16>();

#[derive(Serialize, Deserialize)]
struct WirePublication {
    token: u64,
    frame: Vec<u8>,
    status: Vec<u8>,
    effects: Vec<WireEffect>,
}

#[derive(Serialize, Deserialize)]
enum WireEffect {
    Window(WireWindowCommand),
}

#[derive(Serialize, Deserialize)]
enum WireWindowCommand {
    Minimize,
    Maximize,
    Close,
}

/// Encodes one frame/status/effect publication atomically.
pub fn encode_publication(publication: &AppPublication) -> Result<Vec<u8>, FrameCodecError> {
    if publication.status.frame_token != Some(publication.token) {
        return Err(FrameCodecError::Encode(
            "publication status token does not match publication token".to_owned(),
        ));
    }
    let wire = WirePublication {
        token: publication.token.get(),
        frame: encode_frame(&publication.frame)?,
        status: encode_status(&publication.status)?,
        effects: publication.effects.iter().map(WireEffect::from).collect(),
    };
    let payload =
        postcard::to_allocvec(&wire).map_err(|error| FrameCodecError::Encode(error.to_string()))?;
    let mut bytes = Vec::with_capacity(HEADER_LEN + payload.len());
    bytes.extend_from_slice(&PUBLICATION_MAGIC);
    bytes.extend_from_slice(&PUBLICATION_VERSION.to_le_bytes());
    bytes.extend_from_slice(&payload);
    Ok(bytes)
}

/// Decodes and validates one atomic application publication.
pub fn decode_publication(bytes: &[u8]) -> Result<AppPublication, FrameCodecError> {
    if bytes.len() < HEADER_LEN || bytes[..PUBLICATION_MAGIC.len()] != PUBLICATION_MAGIC {
        return Err(FrameCodecError::InvalidMagic);
    }
    let version = u16::from_le_bytes([bytes[4], bytes[5]]);
    if version != PUBLICATION_VERSION {
        return Err(FrameCodecError::UnsupportedVersion(version));
    }
    let (wire, remaining): (WirePublication, &[u8]) =
        postcard::take_from_bytes(&bytes[HEADER_LEN..])
            .map_err(|error| FrameCodecError::Decode(error.to_string()))?;
    if !remaining.is_empty() {
        return Err(FrameCodecError::TrailingBytes);
    }
    let token = AppFrameToken::new(wire.token)
        .ok_or_else(|| FrameCodecError::Decode("publication token must be non-zero".to_owned()))?;
    let status = decode_status(&wire.status)?;
    if status.frame_token != Some(token) {
        return Err(FrameCodecError::Decode(
            "publication status token mismatch".to_owned(),
        ));
    }
    Ok(AppPublication {
        token,
        frame: decode_frame(&wire.frame)?,
        status,
        effects: wire.effects.into_iter().map(AppEffect::from).collect(),
    })
}

impl From<&AppEffect> for WireEffect {
    fn from(effect: &AppEffect) -> Self {
        match effect {
            AppEffect::Window(command) => Self::Window((*command).into()),
        }
    }
}

impl From<AppEffect> for WireEffect {
    fn from(effect: AppEffect) -> Self {
        Self::from(&effect)
    }
}

impl From<WireEffect> for AppEffect {
    fn from(effect: WireEffect) -> Self {
        match effect {
            WireEffect::Window(command) => Self::Window(command.into()),
        }
    }
}

impl From<WindowCommand> for WireWindowCommand {
    fn from(command: WindowCommand) -> Self {
        match command {
            WindowCommand::Minimize => Self::Minimize,
            WindowCommand::Maximize => Self::Maximize,
            WindowCommand::Close => Self::Close,
        }
    }
}

impl From<WireWindowCommand> for WindowCommand {
    fn from(command: WireWindowCommand) -> Self {
        match command {
            WireWindowCommand::Minimize => Self::Minimize,
            WireWindowCommand::Maximize => Self::Maximize,
            WireWindowCommand::Close => Self::Close,
        }
    }
}

#[cfg(test)]
mod tests {
    use tela_app_session::{AppEffect, AppFrameToken, AppPublication, AppStatus};
    use tela_contract::{UiFrame, Viewport, WindowCommand};

    use super::{decode_publication, encode_publication};

    #[test]
    fn atomic_publication_round_trips_frame_status_and_effects() {
        let token = AppFrameToken::new(7).expect("non-zero token");
        let publication = AppPublication {
            token,
            frame: UiFrame {
                viewport: Viewport {
                    width: 640.0,
                    height: 480.0,
                },
                commands: Vec::new(),
                hit_regions: Vec::new(),
                scroll_bounds: Vec::new(),
            },
            status: AppStatus {
                frame_token: Some(token),
                ..AppStatus::default()
            },
            effects: vec![AppEffect::Window(WindowCommand::Close)],
        };

        let decoded = decode_publication(
            &encode_publication(&publication).expect("encode atomic publication"),
        )
        .expect("decode atomic publication");
        assert_eq!(decoded, publication);
    }

    #[test]
    fn publication_rejects_mismatched_status_token() {
        let token = AppFrameToken::new(7).expect("non-zero token");
        let other = AppFrameToken::new(8).expect("non-zero token");
        let publication = AppPublication {
            token,
            frame: UiFrame {
                viewport: Viewport {
                    width: 1.0,
                    height: 1.0,
                },
                commands: Vec::new(),
                hit_regions: Vec::new(),
                scroll_bounds: Vec::new(),
            },
            status: AppStatus {
                frame_token: Some(other),
                ..AppStatus::default()
            },
            effects: Vec::new(),
        };

        assert!(encode_publication(&publication).is_err());
    }
}
