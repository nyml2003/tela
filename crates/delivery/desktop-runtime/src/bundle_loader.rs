//! Development bundle retrieval, integrity verification, and one-file cache fallback.

use std::{fmt, fs, path::PathBuf, time::Duration};

use tela_bundle::BundleArchive;
use tela_guest_runtime::{MAX_ARCHIVE_BYTES, load_remote_bundle, validate_bundle_archive};

/// Where the usable application package came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BundleSource {
    /// The startup request returned a valid current bundle.
    Network,
    /// The startup request failed, so the last verified local package was used.
    Cache,
}

/// Startup timing values collected without adding a telemetry dependency to the host core.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BundleLoadMetrics {
    /// Time spent fetching the remote index and archive, or zero for cache fallback.
    pub download: Duration,
    /// Size of the compressed archive accepted by the loader.
    pub archive_bytes: usize,
}

/// A verified bundle ready to instantiate in the WASM runtime.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadedBundle {
    /// Decompressed, checksum-validated content.
    pub archive: BundleArchive,
    /// Whether startup used the network or the last local package.
    pub source: BundleSource,
    /// Download/cache timing retained for development diagnostics.
    pub metrics: BundleLoadMetrics,
    /// Cache persistence failure that did not prevent the newly downloaded package from running.
    pub cache_warning: Option<String>,
}

/// A bundle failure suitable for a native shell error dialog or stderr.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BundleLoadError {
    /// The remote index/archive could not be retrieved or verified.
    Network(String),
    /// The cached bundle is absent or no longer valid.
    Cache(String),
    /// Neither the current remote package nor cache can start the app.
    Unavailable {
        /// The remote request or validation failure.
        network: String,
        /// The last-valid cache read or validation failure.
        cache: String,
    },
}

impl fmt::Display for BundleLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Network(message) => {
                write!(formatter, "development bundle request failed: {message}")
            }
            Self::Cache(message) => {
                write!(formatter, "cached development bundle failed: {message}")
            }
            Self::Unavailable { network, cache } => write!(
                formatter,
                "no valid development bundle (network: {network}; cache: {cache})"
            ),
        }
    }
}

impl std::error::Error for BundleLoadError {}

/// One-file cache policy for development application packages.
#[derive(Clone, Debug)]
pub struct BundleLoader {
    cache_path: PathBuf,
}

impl BundleLoader {
    /// Uses `cache_path` as the only last-known-good fallback package.
    pub fn new(cache_path: PathBuf) -> Self {
        Self { cache_path }
    }

    /// Fetches the index and archive once. Any remote failure falls back to the last validated cache.
    pub fn load_with<F>(
        &self,
        index_url: &str,
        mut fetch: F,
    ) -> Result<LoadedBundle, BundleLoadError>
    where
        F: FnMut(&str) -> Result<Vec<u8>, String>,
    {
        match self.load_remote(index_url, &mut fetch) {
            Ok(bundle) => Ok(bundle),
            Err(network) => match self.load_cache() {
                Ok(bundle) => Ok(bundle),
                Err(cache) => Err(BundleLoadError::Unavailable {
                    network: network.to_string(),
                    cache: cache.to_string(),
                }),
            },
        }
    }

    fn load_remote<F>(
        &self,
        index_url: &str,
        fetch: &mut F,
    ) -> Result<LoadedBundle, BundleLoadError>
    where
        F: FnMut(&str) -> Result<Vec<u8>, String>,
    {
        let remote =
            load_remote_bundle(index_url, |url| fetch(url)).map_err(BundleLoadError::Network)?;
        let cache_warning = self.persist_cache(&remote.archive_bytes).err();
        Ok(LoadedBundle {
            archive: remote.archive,
            source: BundleSource::Network,
            metrics: BundleLoadMetrics {
                download: remote.metrics.download,
                archive_bytes: remote.metrics.archive_bytes,
            },
            cache_warning,
        })
    }

    fn load_cache(&self) -> Result<LoadedBundle, BundleLoadError> {
        let bytes = fs::read(&self.cache_path).map_err(|error| {
            BundleLoadError::Cache(format!("read {}: {error}", self.cache_path.display()))
        })?;
        if bytes.len() > MAX_ARCHIVE_BYTES {
            return Err(BundleLoadError::Cache(format!(
                "cache exceeds {} MiB limit",
                MAX_ARCHIVE_BYTES / 1024 / 1024
            )));
        }
        let archive = validate_bundle_archive(&bytes).map_err(BundleLoadError::Cache)?;
        Ok(LoadedBundle {
            archive,
            source: BundleSource::Cache,
            metrics: BundleLoadMetrics {
                download: Duration::ZERO,
                archive_bytes: bytes.len(),
            },
            cache_warning: None,
        })
    }

    fn persist_cache(&self, bytes: &[u8]) -> Result<(), String> {
        let parent = self
            .cache_path
            .parent()
            .ok_or_else(|| "cache path has no parent directory".to_owned())?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("create {}: {error}", parent.display()))?;
        let temporary = self.cache_path.with_extension("tmp");
        fs::write(&temporary, bytes)
            .map_err(|error| format!("write {}: {error}", temporary.display()))?;
        if let Err(error) = fs::rename(&temporary, &self.cache_path) {
            // Windows does not consistently replace an existing target with `rename`; the fallback is
            // only cache persistence, so a brief no-cache window is acceptable after a verified download.
            let _ = fs::remove_file(&self.cache_path);
            fs::rename(&temporary, &self.cache_path).map_err(|retry| {
                format!(
                    "replace {}: {error}; retry: {retry}",
                    self.cache_path.display()
                )
            })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        time::{SystemTime, UNIX_EPOCH},
    };

    use tela_app_abi::ABI_VERSION;
    use tela_bundle::{
        BUNDLE_FORMAT_VERSION, BundleInput, DevelopmentManifest, build_archive, sha256_hex,
    };
    use tela_guest_runtime::resolve_bundle_url;

    use super::*;

    fn temp_cache_path() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("tela-bundle-loader-{unique}.tela"))
    }

    fn archive() -> Vec<u8> {
        build_archive(&BundleInput {
            app_abi: ABI_VERSION,
            app_wasm: b"fake guest".to_vec(),
            assets: BTreeMap::new(),
        })
        .expect("archive")
    }

    #[test]
    fn uses_network_once_then_validates_and_caches() {
        let cache = temp_cache_path();
        let archive = archive();
        let index = serde_json::to_vec(&DevelopmentManifest {
            format_version: BUNDLE_FORMAT_VERSION,
            bundle_id: sha256_hex(&archive),
            bundle_url: "/tela-dev/tela-desktop-guest.tela".to_owned(),
            sha256: sha256_hex(&archive),
            bytes: archive.len() as u64,
            app_abi: ABI_VERSION,
        })
        .expect("index");
        let loader = BundleLoader::new(cache.clone());
        let loaded = loader
            .load_with(
                "http://127.0.0.1:8000/tela-dev/latest.json",
                |url| match url {
                    "http://127.0.0.1:8000/tela-dev/latest.json" => Ok(index.clone()),
                    "http://127.0.0.1:8000/tela-dev/tela-desktop-guest.tela" => Ok(archive.clone()),
                    other => Err(format!("unexpected URL {other}")),
                },
            )
            .expect("network bundle");
        assert_eq!(loaded.source, BundleSource::Network);
        assert_eq!(loaded.archive.app_wasm, b"fake guest");
        assert!(cache.exists());
        let _ = fs::remove_file(cache);
    }

    #[test]
    fn falls_back_to_last_verified_cache() {
        let cache = temp_cache_path();
        let archive = archive();
        fs::write(&cache, &archive).expect("seed cache");
        let loaded = BundleLoader::new(cache.clone())
            .load_with("http://127.0.0.1:8000/tela-dev/latest.json", |_| {
                Err("offline".to_owned())
            })
            .expect("cached bundle");
        assert_eq!(loaded.source, BundleSource::Cache);
        assert_eq!(loaded.metrics.download, Duration::ZERO);
        let _ = fs::remove_file(cache);
    }

    #[test]
    fn resolves_root_relative_bundle_against_remote_origin() {
        assert_eq!(
            resolve_bundle_url(
                "http://host:8000/tela-dev/latest.json",
                "/tela-dev/tela-desktop-guest.tela"
            ),
            Ok("http://host:8000/tela-dev/tela-desktop-guest.tela".to_owned())
        );
    }
}
