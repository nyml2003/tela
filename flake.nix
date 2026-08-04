{
  description = "tela — 通用 UI 基座 Rust workspace 开发环境";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs = { nixpkgs, ... }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
    in {
      devShells.${system}.default = pkgs.mkShell {
        packages = with pkgs; [
          cargo
          clippy
          git
          lld
          nixd
          nodejs
          rust-analyzer
          rustc
          rustfmt
          wasm-bindgen-cli
          (writeShellApplication {
            name = "check";
            runtimeInputs = [ bash cargo clippy rustfmt ];
            text = ''
              set -euo pipefail
              cargo fmt --check
              cargo clippy --all-targets -- -D warnings
              cargo test
              scripts/check-architecture.sh
            '';
          })
        ];

        RUST_BACKTRACE = "1";
        RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
      };
    };
}
