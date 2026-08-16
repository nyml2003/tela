//! Strict retrieval and validation of a current development bundle.

use std::time::{Duration, Instant};

use tela_app_abi::ABI_VERSION;
use tela_bundle::{BundleArchive, DevelopmentManifest, read_archive, sha256_hex};

/// Largest compressed `.tela` archive accepted by a host.
pub const MAX_ARCHIVE_BYTES: usize = 64 * 1024 * 1024;

/// Timing and size values collected while loading one current remote bundle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RemoteBundleMetrics {
    /// Time spent fetching the index and archive.
    pub download: Duration,
    /// Size of the compressed archive accepted by the loader.
    pub archive_bytes: usize,
}

/// A checksum-validated development bundle and the bytes that produced it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteBundle {
    /// Decompressed manifest, WASM guest, and assets.
    pub archive: BundleArchive,
    /// The verified compressed archive, retained only for a shell that elects to cache it.
    pub archive_bytes: Vec<u8>,
    /// Fetch timing retained for host diagnostics.
    pub metrics: RemoteBundleMetrics,
}

/// Fetches exactly the indexed current bundle without any cache fallback.
///
/// The caller owns networking and supplies a closure so this crate stays independent of HTTP,
/// TLS, platform permissions, and cache policy.
pub fn load_remote_bundle<F>(index_url: &str, mut fetch: F) -> Result<RemoteBundle, String>
where
    F: FnMut(&str) -> Result<Vec<u8>, String>,
{
    let started = Instant::now();
    let index = fetch(index_url).map_err(|error| format!("fetch development index: {error}"))?;
    let manifest: DevelopmentManifest = serde_json::from_slice(&index)
        .map_err(|error| format!("invalid development index: {error}"))?;
    manifest
        .validate()
        .map_err(|error| format!("invalid development index: {error}"))?;
    if manifest.app_abi != ABI_VERSION {
        return Err(format!(
            "app ABI mismatch: host={ABI_VERSION}, bundle={}",
            manifest.app_abi
        ));
    }
    if manifest.bytes > MAX_ARCHIVE_BYTES as u64 {
        return Err(format!(
            "bundle exceeds {} MiB limit",
            MAX_ARCHIVE_BYTES / 1024 / 1024
        ));
    }
    let archive_url = resolve_bundle_url(index_url, &manifest.bundle_url)?;
    let archive_bytes = fetch(&archive_url).map_err(|error| format!("fetch bundle: {error}"))?;
    if archive_bytes.len() as u64 != manifest.bytes {
        return Err(format!(
            "archive size mismatch: expected {}, got {}",
            manifest.bytes,
            archive_bytes.len()
        ));
    }
    let actual_hash = sha256_hex(&archive_bytes);
    if actual_hash != manifest.sha256 {
        return Err(format!(
            "archive checksum mismatch: expected {}, got {actual_hash}",
            manifest.sha256
        ));
    }
    let archive = validate_bundle_archive(&archive_bytes)?;
    Ok(RemoteBundle {
        archive,
        metrics: RemoteBundleMetrics {
            download: started.elapsed(),
            archive_bytes: archive_bytes.len(),
        },
        archive_bytes,
    })
}

/// Parses an archive and verifies it uses the current Tela application ABI.
pub fn validate_bundle_archive(bytes: &[u8]) -> Result<BundleArchive, String> {
    if bytes.len() > MAX_ARCHIVE_BYTES {
        return Err(format!(
            "archive exceeds {} MiB limit",
            MAX_ARCHIVE_BYTES / 1024 / 1024
        ));
    }
    let archive = read_archive(bytes).map_err(|error| error.to_string())?;
    if archive.manifest.app_abi != ABI_VERSION {
        return Err(format!(
            "app ABI mismatch: host={ABI_VERSION}, archive={}",
            archive.manifest.app_abi
        ));
    }
    Ok(archive)
}

/// Resolves a manifest URL relative to its absolute development-index URL.
pub fn resolve_bundle_url(index_url: &str, bundle_url: &str) -> Result<String, String> {
    if bundle_url.contains("://") {
        return Ok(bundle_url.to_owned());
    }
    let scheme = index_url
        .find("://")
        .ok_or_else(|| format!("index URL must be absolute: {index_url}"))?;
    let authority_start = scheme + 3;
    let path_start = index_url[authority_start..]
        .find('/')
        .map(|offset| authority_start + offset)
        .unwrap_or(index_url.len());
    if bundle_url.starts_with('/') {
        return Ok(format!("{}{}", &index_url[..path_start], bundle_url));
    }
    let directory_end = index_url.rfind('/').unwrap_or(path_start);
    Ok(format!("{}/{}", &index_url[..directory_end], bundle_url))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use tela_bundle::{BUNDLE_FORMAT_VERSION, BundleInput, build_archive, sha256_hex};

    use super::*;

    fn archive() -> Vec<u8> {
        build_archive(&BundleInput {
            app_abi: ABI_VERSION,
            app_wasm: b"fake guest".to_vec(),
            assets: BTreeMap::new(),
        })
        .expect("archive")
    }

    #[test]
    fn loads_only_the_current_checksum_validated_remote_bundle() {
        let archive = archive();
        let index = serde_json::to_vec(&DevelopmentManifest {
            format_version: BUNDLE_FORMAT_VERSION,
            bundle_id: sha256_hex(&archive),
            bundle_url: "/tela-mobile/mobile-demo.tela".to_owned(),
            sha256: sha256_hex(&archive),
            bytes: archive.len() as u64,
            app_abi: ABI_VERSION,
        })
        .expect("index");
        let loaded =
            load_remote_bundle(
                "http://127.0.0.1:8000/tela-mobile/latest.json",
                |url| match url {
                    "http://127.0.0.1:8000/tela-mobile/latest.json" => Ok(index.clone()),
                    "http://127.0.0.1:8000/tela-mobile/mobile-demo.tela" => Ok(archive.clone()),
                    other => Err(format!("unexpected URL {other}")),
                },
            )
            .expect("remote bundle");
        assert_eq!(loaded.archive.app_wasm, b"fake guest");
        assert_eq!(loaded.archive_bytes, archive);
    }

    #[test]
    fn rejects_remote_failure_without_a_hidden_fallback() {
        let error = load_remote_bundle("http://127.0.0.1:8000/latest.json", |_| {
            Err("offline".to_owned())
        })
        .expect_err("strict remote load must fail");
        assert!(error.contains("offline"));
    }

    #[test]
    fn resolves_root_relative_bundle_against_remote_origin() {
        assert_eq!(
            resolve_bundle_url(
                "http://host:8000/tela-mobile/latest.json",
                "/tela-mobile/mobile-demo.tela"
            ),
            Ok("http://host:8000/tela-mobile/mobile-demo.tela".to_owned())
        );
    }
}
