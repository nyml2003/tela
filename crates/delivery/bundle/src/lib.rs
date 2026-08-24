//! Tela development application bundle format.
//!
//! `ops` writes a small development manifest next to one compressed archive. Platform SDKs fetch
//! the manifest once during startup, verify the archive checksum, then use this crate to reject
//! malformed or path-traversing archive contents before caching them.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod archive;
mod error;
mod manifest;

pub use archive::{BundleArchive, BundleInput, build_archive, read_archive};
pub use error::BundleError;
pub use manifest::{BundleEntry, BundleManifest, DevelopmentManifest, sha256_hex};

/// Archive format version understood by this crate.
pub const BUNDLE_FORMAT_VERSION: u32 = 2;
