//! Build and verify the compressed archive consumed by development SDKs.

use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read, Write},
    path::{Component, Path},
};

use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::{BUNDLE_FORMAT_VERSION, BundleEntry, BundleError, BundleManifest, sha256_hex};

const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_ENTRY_BYTES: u64 = 64 * 1024 * 1024;
const MAX_UNCOMPRESSED_BYTES: u64 = 256 * 1024 * 1024;
const MAX_ASSET_ENTRIES: usize = 1024;

/// Input passed to the deterministic archive builder.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BundleInput {
    /// ABI implemented by `app.wasm`.
    pub app_abi: u32,
    /// Guest application module bytes.
    pub app_wasm: Vec<u8>,
    /// Archive-relative resource names without the automatic `assets/` prefix.
    pub assets: BTreeMap<String, Vec<u8>>,
}

/// Validated application bytes and assets read from an archive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BundleArchive {
    /// Internal manifest.
    pub manifest: BundleManifest,
    /// Guest application bytes.
    pub app_wasm: Vec<u8>,
    /// Resource bytes keyed by their `assets/...` archive path.
    pub assets: BTreeMap<String, Vec<u8>>,
}

/// Builds one compressed archive with a deterministic internal content identifier.
pub fn build_archive(input: &BundleInput) -> Result<Vec<u8>, BundleError> {
    if input.app_abi == 0 {
        return Err(BundleError::InvalidManifest(
            "app_abi must be non-zero".to_owned(),
        ));
    }
    if input.app_wasm.is_empty() {
        return Err(BundleError::InvalidManifest(
            "app.wasm cannot be empty".to_owned(),
        ));
    }
    let manifest = manifest_for(input)?;
    validate_manifest_limits(&manifest)?;
    write_archive(input, &manifest)
}

/// Reads and validates all bytes in a compressed bundle archive.
pub fn read_archive(bytes: &[u8]) -> Result<BundleArchive, BundleError> {
    let mut zip = ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| BundleError::Archive(error.to_string()))?;
    let manifest_bytes = read_required(&mut zip, "manifest.json", MAX_MANIFEST_BYTES)?;
    let manifest: BundleManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|error| BundleError::Json(error.to_string()))?;
    manifest.validate()?;
    validate_manifest_limits(&manifest)?;
    validate_archive_entries(&mut zip, &manifest)?;

    let app_wasm = read_checked(&mut zip, &manifest.app_entry)?;
    let mut assets = BTreeMap::new();
    for entry in &manifest.assets {
        assets.insert(entry.path.clone(), read_checked(&mut zip, entry)?);
    }
    Ok(BundleArchive {
        manifest,
        app_wasm,
        assets,
    })
}

fn manifest_for(input: &BundleInput) -> Result<BundleManifest, BundleError> {
    let mut assets = Vec::with_capacity(input.assets.len());
    for (name, bytes) in &input.assets {
        validate_relative_asset(name)?;
        assets.push(BundleEntry {
            path: format!("assets/{name}"),
            sha256: sha256_hex(bytes),
            bytes: bytes.len() as u64,
        });
    }
    let app_entry = BundleEntry {
        path: "app.wasm".to_owned(),
        sha256: sha256_hex(&input.app_wasm),
        bytes: input.app_wasm.len() as u64,
    };
    let mut canonical = format!("{}:{}:{}", input.app_abi, app_entry.path, app_entry.sha256);
    for entry in &assets {
        canonical.push_str(&format!(":{}:{}", entry.path, entry.sha256));
    }
    Ok(BundleManifest {
        format_version: BUNDLE_FORMAT_VERSION,
        app_abi: input.app_abi,
        bundle_id: sha256_hex(canonical.as_bytes()),
        app_entry,
        assets,
    })
}

fn write_archive(input: &BundleInput, manifest: &BundleManifest) -> Result<Vec<u8>, BundleError> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut zip = ZipWriter::new(&mut cursor);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        zip.start_file("app.wasm", options)
            .map_err(|error| BundleError::Archive(error.to_string()))?;
        zip.write_all(&input.app_wasm)?;
        for (name, bytes) in &input.assets {
            validate_relative_asset(name)?;
            zip.start_file(format!("assets/{name}"), options)
                .map_err(|error| BundleError::Archive(error.to_string()))?;
            zip.write_all(bytes)?;
        }
        let manifest_bytes =
            serde_json::to_vec(manifest).map_err(|error| BundleError::Json(error.to_string()))?;
        zip.start_file("manifest.json", options)
            .map_err(|error| BundleError::Archive(error.to_string()))?;
        zip.write_all(&manifest_bytes)?;
        zip.finish()
            .map_err(|error| BundleError::Archive(error.to_string()))?;
    }
    Ok(cursor.into_inner())
}

fn read_required(
    zip: &mut ZipArchive<Cursor<&[u8]>>,
    path: &str,
    maximum_bytes: u64,
) -> Result<Vec<u8>, BundleError> {
    validate_archive_path(path)?;
    let mut entry = zip
        .by_name(path)
        .map_err(|_| BundleError::MissingEntry(path.to_owned()))?;
    if entry.size() > maximum_bytes {
        return Err(BundleError::InvalidManifest(format!(
            "{path} exceeds {} MiB limit",
            maximum_bytes / 1024 / 1024
        )));
    }
    let capacity = usize::try_from(entry.size()).map_err(|_| {
        BundleError::InvalidManifest(format!("{path} is too large for this platform"))
    })?;
    let mut bytes = Vec::with_capacity(capacity);
    entry.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn read_checked(
    zip: &mut ZipArchive<Cursor<&[u8]>>,
    declared: &BundleEntry,
) -> Result<Vec<u8>, BundleError> {
    validate_archive_path(&declared.path)?;
    if declared.bytes > MAX_ENTRY_BYTES {
        return Err(BundleError::InvalidManifest(format!(
            "{} exceeds {} MiB limit",
            declared.path,
            MAX_ENTRY_BYTES / 1024 / 1024
        )));
    }
    let bytes = read_required(zip, &declared.path, declared.bytes)?;
    let actual = sha256_hex(&bytes);
    if actual != declared.sha256 {
        return Err(BundleError::ChecksumMismatch {
            name: declared.path.clone(),
            expected: declared.sha256.clone(),
            actual,
        });
    }
    if bytes.len() as u64 != declared.bytes {
        return Err(BundleError::InvalidManifest(format!(
            "byte length mismatch for {}",
            declared.path
        )));
    }
    Ok(bytes)
}

fn validate_manifest_limits(manifest: &BundleManifest) -> Result<(), BundleError> {
    if manifest.assets.len() > MAX_ASSET_ENTRIES {
        return Err(BundleError::InvalidManifest(format!(
            "bundle contains more than {MAX_ASSET_ENTRIES} assets"
        )));
    }
    let mut total = manifest.app_entry.bytes;
    if total > MAX_ENTRY_BYTES {
        return Err(BundleError::InvalidManifest(format!(
            "app.wasm exceeds {} MiB limit",
            MAX_ENTRY_BYTES / 1024 / 1024
        )));
    }
    for asset in &manifest.assets {
        if asset.bytes > MAX_ENTRY_BYTES {
            return Err(BundleError::InvalidManifest(format!(
                "{} exceeds {} MiB limit",
                asset.path,
                MAX_ENTRY_BYTES / 1024 / 1024
            )));
        }
        total = total
            .checked_add(asset.bytes)
            .ok_or_else(|| BundleError::InvalidManifest("bundle byte count overflow".to_owned()))?;
    }
    if total > MAX_UNCOMPRESSED_BYTES {
        return Err(BundleError::InvalidManifest(format!(
            "bundle exceeds {} MiB uncompressed limit",
            MAX_UNCOMPRESSED_BYTES / 1024 / 1024
        )));
    }
    Ok(())
}

fn validate_archive_entries(
    zip: &mut ZipArchive<Cursor<&[u8]>>,
    manifest: &BundleManifest,
) -> Result<(), BundleError> {
    let mut expected =
        BTreeSet::from(["manifest.json".to_owned(), manifest.app_entry.path.clone()]);
    expected.extend(manifest.assets.iter().map(|asset| asset.path.clone()));
    if zip.len() != expected.len() {
        return Err(BundleError::Archive(format!(
            "expected {} archive entries, found {}",
            expected.len(),
            zip.len()
        )));
    }
    for index in 0..zip.len() {
        let entry = zip
            .by_index(index)
            .map_err(|error| BundleError::Archive(error.to_string()))?;
        let name = entry.name().to_owned();
        if !expected.remove(&name) {
            return Err(BundleError::Archive(format!(
                "unexpected or duplicate entry: {name}"
            )));
        }
    }
    if let Some(name) = expected.into_iter().next() {
        return Err(BundleError::MissingEntry(name));
    }
    Ok(())
}

fn validate_relative_asset(path: &str) -> Result<(), BundleError> {
    if path.is_empty() {
        return Err(BundleError::InvalidPath(path.to_owned()));
    }
    validate_archive_path(path)
}

fn validate_archive_path(path: &str) -> Result<(), BundleError> {
    if path.contains('\\') {
        return Err(BundleError::InvalidPath(path.to_owned()));
    }
    let parsed = Path::new(path);
    if parsed.is_absolute()
        || parsed
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(BundleError::InvalidPath(path.to_owned()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> BundleInput {
        BundleInput {
            app_abi: 1,
            app_wasm: b"\\0asm-demo".to_vec(),
            assets: BTreeMap::from([("icons/readme.txt".to_owned(), b"icon-data".to_vec())]),
        }
    }

    #[test]
    fn round_trips_a_compressed_bundle() {
        let archive = build_archive(&input()).expect("build archive");
        let decoded = read_archive(&archive).expect("read archive");
        assert_eq!(decoded.app_wasm, input().app_wasm);
        assert_eq!(decoded.assets["assets/icons/readme.txt"], b"icon-data");
    }

    #[test]
    fn rejects_path_traversal_before_writing_an_archive() {
        let mut invalid = input();
        invalid.assets.insert("../outside".to_owned(), vec![1]);
        assert!(matches!(
            build_archive(&invalid),
            Err(BundleError::InvalidPath(_))
        ));
    }

    #[test]
    fn rejects_unlisted_archive_entries() {
        let input = input();
        let manifest = manifest_for(&input).expect("manifest");
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut cursor);
            let options =
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
            zip.start_file("app.wasm", options).expect("app entry");
            zip.write_all(&input.app_wasm).expect("app bytes");
            zip.start_file("assets/icons/readme.txt", options)
                .expect("asset entry");
            zip.write_all(&input.assets["icons/readme.txt"])
                .expect("asset bytes");
            zip.start_file("manifest.json", options)
                .expect("manifest entry");
            zip.write_all(&serde_json::to_vec(&manifest).expect("manifest bytes"))
                .expect("write manifest");
            zip.start_file("unexpected.bin", options)
                .expect("extra entry");
            zip.write_all(b"not declared").expect("extra bytes");
            zip.finish().expect("finish archive");
        }
        assert!(matches!(
            read_archive(&cursor.into_inner()),
            Err(BundleError::Archive(_))
        ));
    }

    #[test]
    fn rejects_manifest_with_noncanonical_content_identifier() {
        let mut manifest = manifest_for(&input()).expect("manifest");
        manifest.bundle_id = "0".repeat(64);
        assert!(matches!(
            manifest.validate(),
            Err(BundleError::InvalidManifest(_))
        ));
    }
}
