{
  description = "tela — 通用 UI 基座 Rust workspace 开发环境";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs = { nixpkgs, ... }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-darwin"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in {
      devShells = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
          commonPackages = with pkgs; [
            cargo
            clippy
            python3Packages.fonttools
            git
            nixd
            nodejs
            pnpm
            rust-analyzer
            rustc
            rustfmt
            # wasm-bindgen-cli must match the Rust wasm-bindgen schema in Cargo.lock.
            wasm-bindgen-cli_0_2_126
          ];
          checkCommand = pkgs.writeShellApplication {
            name = "check";
            # 统一走 ops 工作流（DDD，零运行时依赖；含 TS 版依赖方向检查，
            # 见 ops/README.md）。ops check 内部调 cargo fmt/clippy/test。
            runtimeInputs = with pkgs; [ bash cargo nodejs ];
            text = ''
              set -euo pipefail
              node ops/src/interface/cli.ts check
            '';
          };
          commonShellHook = ''
            # ops 工作流命令（DDD CLI，Node 24 直接跑 TS，见 ops/README.md）。
            # 优先用用户级安装（~/.local/bin/ops）；未安装时回退到项目内脚本。
            if ! command -v ops >/dev/null 2>&1; then
              alias ops="node $(git rev-parse --show-toplevel 2>/dev/null || echo "$(pwd)")/ops/src/interface/cli.ts"
            fi
            echo "tela dev shell ready (cargo $(cargo --version | cut -d' ' -f2), node $(node --version), pnpm $(pnpm --version))"
          '';
        in {
          default = if pkgs.stdenv.isLinux then
            let
              win32Cargo = pkgs.pkgsCross.mingwW64.buildPackages.cargo;
              win32Clippy = pkgs.pkgsCross.mingwW64.buildPackages.clippy;
              # Rust 的 x86_64-pc-windows-gnu std 仍请求 libpthread.a；当前 MinGW GCC 默认使用
              # mcfgthread，因此显式保留 GNU pthread 兼容 archive 给这个交叉命令。
              win32Pthreads = pkgs.pkgsCross.mingwW64.windows.pthreads;
              cargoWin32 = pkgs.writeShellApplication {
                name = "cargo-win32";
                runtimeInputs = [
                  win32Cargo
                  win32Clippy
                  win32Pthreads
                  pkgs.pkgsCross.mingwW64.stdenv.cc
                ];
                text = ''
                  export RUSTFLAGS="-L native=${win32Pthreads}/lib''${RUSTFLAGS:+ ''${RUSTFLAGS}}"
                  exec ${win32Cargo}/bin/cargo "$@"
                '';
              };
            in pkgs.mkShell {
              packages = commonPackages ++ (with pkgs; [
                lld
                mesa
                vulkan-loader
                # Win32 开发壳：在 WSL 中交叉链接 x86_64-pc-windows-gnu 工件。
                pkgsCross.mingwW64.stdenv.cc
                cargoWin32
                checkCommand
              ]);

              shellHook = commonShellHook + ''
                # lavapipe（llvmpipe Vulkan）：wgpu 离屏渲染测试（tela-render-wgpu/tests/render_test.rs）
                export VK_DRIVER_FILES="${pkgs.mesa}/share/vulkan/icd.d/lvp_icd.x86_64.json"
                export LD_LIBRARY_PATH="${pkgs.vulkan-loader}/lib:''${LD_LIBRARY_PATH:-}"
              '';

              RUST_BACKTRACE = "1";
              RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
            }
          else pkgs.mkShell {
            packages = commonPackages ++ [
              checkCommand
              # wasm32 目标链接器（wasm-ld）：与 Linux 壳保持同源，保证跨平台 bundle 一致。
              pkgs.lld
            ];

            # AppKit/Metal 与 aarch64-apple-darwin stdlib/SDK 由本机 macOS 和 Xcode Command
            # Line Tools 提供。开发壳只在本机链接，不把 Apple SDK 伪装成 Nix 交叉工具链。
            shellHook = commonShellHook;

            MACOSX_DEPLOYMENT_TARGET = "14.0";
            RUST_BACKTRACE = "1";
            RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
          };
        });
    };
}
