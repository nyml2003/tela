use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    println!("cargo:rerun-if-env-changed=TELA_BUILD_ID");
    println!("cargo:rerun-if-env-changed=TELA_APP_BUILD_ID");
    println!("cargo:rerun-if-env-changed=TELA_BUNDLE_BUILD_ID");

    let generated = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is before the Unix epoch")
        .as_millis()
        .checked_rem(u32::MAX as u128)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value != 0)
        .unwrap_or(1);
    let shared = std::env::var("TELA_BUILD_ID")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value != 0)
        .unwrap_or(generated);
    let app = std::env::var("TELA_APP_BUILD_ID")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value != 0)
        .unwrap_or(shared);
    let bundle = std::env::var("TELA_BUNDLE_BUILD_ID")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value != 0)
        .unwrap_or(shared);

    println!("cargo:rustc-env=TELA_APP_BUILD_ID={app}");
    println!("cargo:rustc-env=TELA_BUNDLE_BUILD_ID={bundle}");
}
