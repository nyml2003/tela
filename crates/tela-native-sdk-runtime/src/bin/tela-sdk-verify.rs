//! Headless command-line verifier for a Tela development bundle.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::{env, path::PathBuf};

fn main() {
    let result = match env::args().skip(1).collect::<Vec<_>>().as_slice() {
        [path] => tela_native_sdk_runtime::verify_bundle(&PathBuf::from(path)),
        _ => Err("usage: tela-sdk-verify <bundle.tela>".to_owned()),
    };
    match result {
        Ok(verification) => eprintln!("tela-sdk-verify: {verification}"),
        Err(error) => {
            eprintln!("tela-sdk-verify: {error}");
            std::process::exit(1);
        }
    }
}
