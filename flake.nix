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
          python3Packages.fonttools
          git
          lld
          mesa
          nixd
          nodejs
          pnpm
          rust-analyzer
          rustc
          rustfmt
          vulkan-loader
          # wasm-bindgen-cli must match the Rust wasm-bindgen schema in Cargo.lock.
          wasm-bindgen-cli_0_2_126
          (writeShellApplication {
            name = "check";
            # 统一走 ops 工作流（DDD，零运行时依赖；含 TS 版依赖方向检查，
            # 见 ops/README.md）。ops check 内部调 cargo fmt/clippy/test。
            runtimeInputs = [ bash cargo nodejs ];
            text = ''
              set -euo pipefail
              node ops/src/interface/cli.ts check
            '';
          })
        ];

        shellHook = ''
          # ops 工作流命令（DDD CLI，Node 24 直接跑 TS，见 ops/README.md）。
          # 优先用用户级安装（~/.local/bin/ops）；未安装时回退到项目内脚本。
          if ! command -v ops >/dev/null 2>&1; then
            alias ops="node $(git rev-parse --show-toplevel 2>/dev/null || echo "$(pwd)")/ops/src/interface/cli.ts"
          fi
          # lavapipe（llvmpipe Vulkan）：wgpu 离屏渲染测试（tela-render-wgpu/tests/render_test.rs）
          export VK_DRIVER_FILES="${pkgs.mesa}/share/vulkan/icd.d/lvp_icd.x86_64.json"
          export LD_LIBRARY_PATH="${pkgs.vulkan-loader}/lib:''${LD_LIBRARY_PATH:-}"
          echo "tela dev shell ready (cargo $(cargo --version | cut -d' ' -f2), node $(node --version), pnpm $(pnpm --version))"
        '';

        RUST_BACKTRACE = "1";
        RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
      };
    };
}
