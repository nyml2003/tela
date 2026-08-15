//! JSON manifests for a compressed development bundle and its remote index.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{BUNDLE_FORMAT_VERSION, BundleError};

/// One content-addressed file declared inside a bundle archive.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleEntry {
    /// Archive-relative path.
    pub path: String,
    /// SHA-256 hex digest of the uncompressed content.
    pub sha256: String,
    /// Uncompressed content length.
    pub bytes: u64,
}

/// Manifest stored as `manifest.json` inside every archive.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleManifest {
    /// Bundle archive protocol version.
    pub format_version: u32,
    /// Application WASM ABI required by this bundle.
    pub app_abi: u32,
    /// Stable identifier derived from the guest module and declared resource hashes.
    pub bundle_id: String,
    /// Application module path, currently always `app.wasm`.
    pub app_entry: BundleEntry,
    /// Optional static resource entries under `assets/`.
    pub assets: Vec<BundleEntry>,
}

impl BundleManifest {
    /// Validates invariant fields before an SDK accepts the archive.
    pub fn validate(&self) -> Result<(), BundleError> {
        if self.format_version != BUNDLE_FORMAT_VERSION {
            return Err(BundleError::InvalidManifest(format!(
                "unsupported format version {}",
                self.format_version
            )));
        }
        if self.app_abi == 0 {
            return Err(BundleError::InvalidManifest(
                "app_abi must be non-zero".to_owned(),
            ));
        }
        if self.bundle_id.len() != 64
            || !self.bundle_id.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(BundleError::InvalidManifest(
                "bundle_id must be a SHA-256 hex digest".to_owned(),
            ));
        }
        validate_entry(&self.app_entry, false)?;
        let mut canonical = format!(
            "{}:{}:{}",
            self.app_abi, self.app_entry.path, self.app_entry.sha256
        );
        let mut previous_path: Option<&str> = None;
        for asset in &self.assets {
            validate_entry(asset, true)?;
            if previous_path.is_some_and(|previous| previous >= asset.path.as_str()) {
                return Err(BundleError::InvalidManifest(
                    "assets must have unique paths in lexical order".to_owned(),
                ));
            }
            canonical.push_str(&format!(":{}:{}", asset.path, asset.sha256));
            previous_path = Some(&asset.path);
        }
        if self.bundle_id != sha256_hex(canonical.as_bytes()) {
            return Err(BundleError::InvalidManifest(
                "bundle_id does not match declared content".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Small remote index fetched by a development SDK at application startup.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevelopmentManifest {
    /// Remote manifest protocol version.
    pub format_version: u32,
    /// Bundle content id.
    pub bundle_id: String,
    /// Relative or absolute URL to the one compressed archive.
    pub bundle_url: String,
    /// SHA-256 of the compressed archive bytes.
    pub sha256: String,
    /// Compressed archive size.
    pub bytes: u64,
    /// Required application WASM ABI.
    pub app_abi: u32,
}

impl DevelopmentManifest {
    /// Validates an index before an SDK sends the archive request.
    pub fn validate(&self) -> Result<(), BundleError> {
        if self.format_version != BUNDLE_FORMAT_VERSION {
            return Err(BundleError::InvalidManifest(format!(
                "unsupported development manifest version {}",
                self.format_version
            )));
        }
        for (name, value) in [("bundle_id", &self.bundle_id), ("sha256", &self.sha256)] {
            if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(BundleError::InvalidManifest(format!(
                    "{name} must be a SHA-256 hex digest"
                )));
            }
        }
        if self.bundle_id != self.sha256 {
            return Err(BundleError::InvalidManifest(
                "bundle_id must equal archive sha256".to_owned(),
            ));
        }
        if self.bundle_url.is_empty() {
            return Err(BundleError::InvalidManifest(
                "bundle_url cannot be empty".to_owned(),
            ));
        }
        if self.app_abi == 0 {
            return Err(BundleError::InvalidManifest(
                "app_abi must be non-zero".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Returns a lowercase SHA-256 digest for bytes.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn validate_entry(entry: &BundleEntry, asset: bool) -> Result<(), BundleError> {
    let expected = if asset { "assets/" } else { "app.wasm" };
    if asset {
        if !entry.path.starts_with(expected) || entry.path.len() <= expected.len() {
            return Err(BundleError::InvalidManifest(format!(
                "asset path must start with {expected}"
            )));
        }
    } else if entry.path != expected {
        return Err(BundleError::InvalidManifest(
            "app entry must be app.wasm".to_owned(),
        ));
    }
    if entry.sha256.len() != 64 || !entry.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(BundleError::InvalidManifest(format!(
            "invalid SHA-256 for {}",
            entry.path
        )));
    }
    Ok(())
}
