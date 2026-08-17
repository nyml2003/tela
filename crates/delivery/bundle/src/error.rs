//! Bundle construction and validation failures.

use std::fmt;

/// An archive or its manifest is invalid for execution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BundleError {
    /// A required field is missing or uses an unsupported protocol version.
    InvalidManifest(String),
    /// A zip path could escape the intended bundle root.
    InvalidPath(String),
    /// The archive is missing an entry declared by its manifest.
    MissingEntry(String),
    /// Bytes do not match their declared SHA-256 checksum.
    ChecksumMismatch {
        /// Bundle or entry name.
        name: String,
        /// Expected SHA-256 hex digest.
        expected: String,
        /// Actual SHA-256 hex digest.
        actual: String,
    },
    /// A byte stream could not be read as an archive.
    Archive(String),
    /// A JSON document could not be encoded or decoded.
    Json(String),
    /// An underlying file operation failed.
    Io(String),
}

impl fmt::Display for BundleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidManifest(message) => write!(f, "invalid Tela bundle manifest: {message}"),
            Self::InvalidPath(path) => write!(f, "invalid Tela bundle path: {path}"),
            Self::MissingEntry(path) => write!(f, "Tela bundle is missing required entry: {path}"),
            Self::ChecksumMismatch {
                name,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "checksum mismatch for {name}: expected {expected}, got {actual}"
                )
            }
            Self::Archive(message) => write!(f, "invalid Tela bundle archive: {message}"),
            Self::Json(message) => write!(f, "invalid Tela bundle JSON: {message}"),
            Self::Io(message) => write!(f, "Tela bundle I/O failed: {message}"),
        }
    }
}

impl std::error::Error for BundleError {}

impl From<std::io::Error> for BundleError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value.to_string())
    }
}
