//! Errors raised while converting or decoding a portable frame.

use std::fmt;

/// A frame cannot be transported or decoded by the current ABI version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrameCodecError {
    /// `DrawPayload::Custom` contains a Rust trait object and deliberately has no portable wire
    /// representation.
    UnsupportedCustomDraw,
    /// Packet magic does not identify a Tela frame packet.
    InvalidMagic,
    /// The packet version is newer or older than this runtime understands.
    UnsupportedVersion(u16),
    /// The packet payload could not be encoded.
    Encode(String),
    /// The packet payload could not be decoded.
    Decode(String),
    /// A valid payload was followed by unexpected bytes.
    TrailingBytes,
}

impl fmt::Display for FrameCodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedCustomDraw => {
                f.write_str("DrawPayload::Custom cannot cross the Tela app ABI")
            }
            Self::InvalidMagic => f.write_str("invalid Tela frame packet magic"),
            Self::UnsupportedVersion(version) => {
                write!(f, "unsupported Tela frame packet version {version}")
            }
            Self::Encode(error) => write!(f, "could not encode Tela frame: {error}"),
            Self::Decode(error) => write!(f, "could not decode Tela frame: {error}"),
            Self::TrailingBytes => f.write_str("Tela frame packet contains trailing bytes"),
        }
    }
}

impl std::error::Error for FrameCodecError {}
