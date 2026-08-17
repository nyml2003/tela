//! Small packer invoked by `ops build bundle`.

#![forbid(unsafe_code)]

use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

use tela_app_abi::ABI_VERSION;
use tela_bundle::{
    BUNDLE_FORMAT_VERSION, BundleInput, DevelopmentManifest, build_archive, read_archive,
    sha256_hex,
};

fn main() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.len() != 4 && args.len() != 5 {
        return Err(
            "usage: tela-bundle <app.wasm> <bundle.zip> <latest.json> <bundle-url> [assets-dir]"
                .to_owned(),
        );
    }
    let app_path = PathBuf::from(&args[0]);
    let archive_path = PathBuf::from(&args[1]);
    let manifest_path = PathBuf::from(&args[2]);
    let bundle_url = args[3].clone();
    let assets = if let Some(path) = args.get(4) {
        read_assets(Path::new(path))?
    } else {
        BTreeMap::new()
    };
    let input = BundleInput {
        app_abi: tela_app_abi_version(),
        app_wasm: fs::read(&app_path)
            .map_err(|error| format!("read {}: {error}", app_path.display()))?,
        assets,
    };
    let archive = build_archive(&input).map_err(|error| error.to_string())?;
    read_archive(&archive).map_err(|error| error.to_string())?;
    let archive_hash = sha256_hex(&archive);
    fs::write(&archive_path, &archive)
        .map_err(|error| format!("write {}: {error}", archive_path.display()))?;
    let manifest = DevelopmentManifest {
        format_version: BUNDLE_FORMAT_VERSION,
        bundle_id: archive_hash.clone(),
        bundle_url,
        sha256: archive_hash,
        bytes: archive.len() as u64,
        app_abi: tela_app_abi_version(),
    };
    manifest.validate().map_err(|error| error.to_string())?;
    let json = serde_json::to_vec_pretty(&manifest).map_err(|error| error.to_string())?;
    fs::write(&manifest_path, json)
        .map_err(|error| format!("write {}: {error}", manifest_path.display()))?;
    Ok(())
}

fn tela_app_abi_version() -> u32 {
    ABI_VERSION
}

fn read_assets(root: &Path) -> Result<BTreeMap<String, Vec<u8>>, String> {
    if !root.exists() {
        return Ok(BTreeMap::new());
    }
    let mut files = Vec::new();
    collect_files(root, root, &mut files)?;
    let mut assets = BTreeMap::new();
    for (relative, absolute) in files {
        assets.insert(
            relative,
            fs::read(&absolute).map_err(|error| format!("read {}: {error}", absolute.display()))?,
        );
    }
    Ok(assets)
}

fn collect_files(
    root: &Path,
    current: &Path,
    files: &mut Vec<(String, PathBuf)>,
) -> Result<(), String> {
    for entry in
        fs::read_dir(current).map_err(|error| format!("read {}: {error}", current.display()))?
    {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, files)?;
        } else if path.is_file() {
            let relative = path.strip_prefix(root).map_err(|error| error.to_string())?;
            let name = relative.to_string_lossy().replace('\\', "/");
            files.push((name, path));
        }
    }
    Ok(())
}
