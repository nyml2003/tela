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
          # nixpkgs revision is the single Rust release source for every product shell and
          # for the private rustup toolchains used by Android/iOS wrappers.
          rustToolchain = pkgs.rustc.version;
          opsCommand = pkgs.writeShellApplication {
            name = "ops";
            runtimeInputs = [ pkgs.bash pkgs.nodejs ];
            text = ''
              set -euo pipefail
              root="$PWD"

              while true; do
                if [ -f "$root/ops/src/interface/cli.ts" ]; then
                  exec node "$root/ops/src/interface/cli.ts" "$@"
                fi

                if [ "$root" = "/" ]; then
                  break
                fi

                root="''${root%/*}"
                if [ -z "$root" ]; then
                  root="/"
                fi
              done

              printf '%s\n' "ops: could not locate ops/src/interface/cli.ts from $PWD" >&2
              exit 1
            '';
          };
          # 所有产品共享同一套 Rust/ops 基线；专属工具必须在各 Product shell 中显式加入。
          basePackages = with pkgs; [
            cargo
            clippy
            nodejs
            rust-analyzer
            rustc
            rustfmt
            opsCommand
          ];
          checkCommand = pkgs.writeShellApplication {
            name = "check";
            # 统一走 ops 工作流（DDD，零运行时依赖；含 TS 版依赖方向检查，
            # 见 ops/README.md）。ops check 内部调 cargo fmt/clippy/test。
            runtimeInputs = [ pkgs.bash pkgs.cargo opsCommand ];
            text = ''
              set -euo pipefail
              exec ${opsCommand}/bin/ops check
            '';
          };
          corePackages = basePackages ++ [ checkCommand ];
          webviewPackages = corePackages ++ (with pkgs; [
            pnpm
            # wasm-bindgen-cli must match the Rust wasm-bindgen schema in Cargo.lock.
            wasm-bindgen-cli_0_2_126
            lld
          ]);
          assetsPackages = corePackages ++ [ pkgs.python3Packages.fonttools ];
          # Android 开发不从当前 nixpkgs pin 拉 Android SDK/NDK：这些包会回退到
          # dl.google.com，且该 pin 的交叉 Rust 不能命中二进制缓存。工具链保存在
          # 项目专属缓存；Windows Android Studio 提供平台和 NDK，Linux build-tools 单独缓存。
          androidBootstrap = pkgs.writeShellScriptBin "tela-android-bootstrap" ''
            set -euo pipefail

            toolchain_root="''${XDG_CACHE_HOME:-$HOME/.cache}/tela/android"
            rustup_home="$toolchain_root/rustup"
            cargo_home="$toolchain_root/cargo"
            jdk_home="$toolchain_root/jdk"
            gradle_home="$toolchain_root/gradle"
            sdk_home="$toolchain_root/sdk"
            windows_sdk_root="''${TELA_ANDROID_WINDOWS_SDK_ROOT:-}"

            require_command() {
              if ! command -v "$1" >/dev/null 2>&1; then
                printf 'tela-android-bootstrap: missing required command: %s\n' "$1" >&2
                exit 1
              fi
            }
            require_command curl
            require_command tar
            require_command unzip
            require_command sha256sum
            require_command sha1sum
            require_command mktemp

            if [ -z "$windows_sdk_root" ]; then
              printf '%s\n' 'Windows Android SDK was not found. Install Android Studio SDK API 36 and reopen nix develop .#android.' >&2
              exit 1
            fi
            windows_platform="$windows_sdk_root/platforms/android-36"
            windows_ndk="$windows_sdk_root/ndk/27.1.12297006"
            windows_platform_tools="$windows_sdk_root/platform-tools"
            if [ ! -f "$windows_platform/android.jar" ] || [ ! -f "$windows_ndk/source.properties" ] || [ ! -f "$windows_platform_tools/source.properties" ]; then
              printf '%s\n' 'Windows Android SDK needs platform android-36, platform-tools, and NDK 27.1.12297006.' >&2
              exit 1
            fi

            mkdir -p "$toolchain_root"
            if [ ! -x "$cargo_home/bin/rustup" ]; then
              installer="$toolchain_root/rustup-init"
              printf '%s\n' 'Downloading the project-local Linux Rust toolchain from rsproxy...'
              curl --fail --location --retry 3 --retry-delay 2 \
                --output "$installer" \
                https://rsproxy.cn/rustup/dist/x86_64-unknown-linux-gnu/rustup-init
              chmod +x "$installer"
              RUSTUP_HOME="$rustup_home" \
                CARGO_HOME="$cargo_home" \
                RUSTUP_DIST_SERVER="https://rsproxy.cn" \
                RUSTUP_UPDATE_ROOT="https://rsproxy.cn/rustup" \
                "$installer" -y --profile minimal --default-toolchain ${rustToolchain} --no-modify-path
            fi
            if ! RUSTUP_HOME="$rustup_home" CARGO_HOME="$cargo_home" \
              "$cargo_home/bin/rustup" run ${rustToolchain} rustc --version >/dev/null 2>&1; then
              RUSTUP_HOME="$rustup_home" \
                CARGO_HOME="$cargo_home" \
                RUSTUP_DIST_SERVER="https://rsproxy.cn" \
                RUSTUP_UPDATE_ROOT="https://rsproxy.cn/rustup" \
                "$cargo_home/bin/rustup" toolchain install ${rustToolchain} --profile minimal
            fi
            RUSTUP_HOME="$rustup_home" \
              CARGO_HOME="$cargo_home" \
              RUSTUP_DIST_SERVER="https://rsproxy.cn" \
              RUSTUP_UPDATE_ROOT="https://rsproxy.cn/rustup" \
              "$cargo_home/bin/rustup" target add aarch64-linux-android --toolchain ${rustToolchain}

            if [ ! -x "$jdk_home/bin/java" ]; then
              archive="$toolchain_root/OpenJDK17U-jdk_x64_linux_hotspot_17.0.20_8.tar.gz"
              expected_jdk_sha256="be7668bc030d578b83d6d5ef9221d6d6729bbbca8cf94a7d52e16ac68b5a5a35"
              # TUNA 的镜像实测可用；hash 来自 Adoptium jdk-17.0.20+8 release API。
              printf '%s\n' 'Downloading the project-local Linux JDK 17 from the TUNA mirror...'
              curl --fail --location --retry 3 --retry-delay 2 \
                --output "$archive" \
                https://mirrors.tuna.tsinghua.edu.cn/Adoptium/17/jdk/x64/linux/OpenJDK17U-jdk_x64_linux_hotspot_17.0.20_8.tar.gz
              read -r actual_jdk_sha256 _ < <(sha256sum "$archive")
              if [ "$actual_jdk_sha256" != "$expected_jdk_sha256" ]; then
                printf 'JDK checksum mismatch: expected %s, got %s\n' "$expected_jdk_sha256" "$actual_jdk_sha256" >&2
                exit 1
              fi
              mkdir -p "$jdk_home"
              tar --extract --gzip --file "$archive" --strip-components=1 --directory "$jdk_home"
            fi

            # Gradle runs in Linux and therefore cannot execute Android Studio's *.exe build tools.
            # Keep the platform jar and NDK on the Windows SDK, but assemble a Linux SDK view for AGP.
            mkdir -p "$sdk_home/platforms" "$sdk_home/ndk"
            ln -sfn "$windows_platform" "$sdk_home/platforms/android-36"
            ln -sfn "$windows_ndk" "$sdk_home/ndk/27.1.12297006"
            ln -sfn "$windows_platform_tools" "$sdk_home/platform-tools"
            if [ -d "$windows_sdk_root/licenses" ]; then
              ln -sfn "$windows_sdk_root/licenses" "$sdk_home/licenses"
            fi
            build_tools_home="$sdk_home/build-tools/36.0.0"
            if [ ! -x "$build_tools_home/aapt2" ]; then
              archive="$toolchain_root/build-tools_r36_linux.zip"
              expected_build_tools_sha1="b0b6376977657e8ad9b969bacf4093601da2c6fb"
              actual_build_tools_sha1=""
              if [ -f "$archive" ]; then
                read -r actual_build_tools_sha1 _ < <(sha1sum "$archive")
              fi
              if [ "$actual_build_tools_sha1" != "$expected_build_tools_sha1" ]; then
                printf '%s\n' 'Downloading Linux Android build-tools 36.0.0 directly from dl.google.com...'
                # The local proxy has repeatedly stalled this artifact. Bypass it only for this
                # checksum-pinned download; the setting is scoped to this curl invocation.
                direct_no_proxy="''${NO_PROXY:+$NO_PROXY,}dl.google.com"
                if [ -f "$archive" ]; then
                  NO_PROXY="$direct_no_proxy" no_proxy="$direct_no_proxy" \
                    curl --fail --location --retry 3 --retry-delay 2 --continue-at - \
                    --output "$archive" \
                    https://dl.google.com/android/repository/build-tools_r36_linux.zip
                else
                  NO_PROXY="$direct_no_proxy" no_proxy="$direct_no_proxy" \
                    curl --fail --location --retry 3 --retry-delay 2 \
                    --output "$archive" \
                    https://dl.google.com/android/repository/build-tools_r36_linux.zip
                fi
                read -r actual_build_tools_sha1 _ < <(sha1sum "$archive")
              fi
              if [ "$actual_build_tools_sha1" != "$expected_build_tools_sha1" ]; then
                printf 'Android build-tools checksum mismatch: expected %s, got %s\n' "$expected_build_tools_sha1" "$actual_build_tools_sha1" >&2
                exit 1
              fi
              extract_root="$(mktemp -d "$toolchain_root/build-tools-r36-linux.XXXXXX")"
              trap 'rm -rf "$extract_root"' EXIT
              unzip -q "$archive" -d "$extract_root"
              # Google's archive keeps a historical android-16 top-level name even for r36.
              # Identify the extracted package by its Linux aapt2 rather than that unstable name.
              extracted_build_tools="$(find "$extract_root" -mindepth 1 -maxdepth 1 -type d -exec sh -c 'test -x "$1/aapt2"' _ {} \; -print -quit)"
              if [ -z "$extracted_build_tools" ]; then
                printf 'Android build-tools archive has no Linux aapt2: %s\n' "$archive" >&2
                exit 1
              fi
              mkdir -p "$(dirname "$build_tools_home")"
              rm -rf "$build_tools_home"
              mv "$extracted_build_tools" "$build_tools_home"
              rm -rf "$extract_root"
              trap - EXIT
            fi

            # Android Studio 通常已经缓存了 Gradle 9.1。没有时才下载一份项目私有副本。
            if [ ! -x "''${TELA_ANDROID_GRADLE:-}" ] && [ ! -x "$gradle_home/gradle-9.1.0/bin/gradle" ]; then
              archive="$toolchain_root/gradle-9.1.0-bin.zip"
              printf '%s\n' 'Downloading the project-local Gradle 9.1 distribution...'
              curl --fail --location --retry 3 --retry-delay 2 \
                --output "$archive" \
                https://services.gradle.org/distributions/gradle-9.1.0-bin.zip
              mkdir -p "$gradle_home"
              unzip -q "$archive" -d "$gradle_home"
            fi

            printf '%s\n' 'Tela Android bootstrap complete.'
          '';
          androidCargo = pkgs.writeShellScriptBin "tela-android-cargo" ''
            set -euo pipefail

            toolchain_root="''${TELA_ANDROID_TOOLCHAIN_ROOT:-''${XDG_CACHE_HOME:-$HOME/.cache}/tela/android}"
            cargo_home="''${TELA_ANDROID_CARGO_HOME:-$toolchain_root/cargo}"
            if [ ! -x "$cargo_home/bin/cargo" ]; then
              printf '%s\n' 'Android Rust toolchain is absent. Run: nix develop .#android --command tela-android-bootstrap' >&2
              exit 1
            fi
            if [ ! -x "''${CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER:-}" ]; then
              printf '%s\n' 'Windows Android NDK r27b was not found. Open Android Studio and install NDK 27.1.12297006.' >&2
              exit 1
            fi
            android_clang="$CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER"
            android_bin_dir="''${android_clang%/*}"
            # cc-rs looks up target-specific variables before CXX. Nix's host shell exports CXX,
            # so make the Android tool selection explicit for android-activity and other C/C++ deps.
            export CC_aarch64_linux_android="$android_clang"
            export CXX_aarch64_linux_android="$android_clang++"
            export AR_aarch64_linux_android="$android_bin_dir/llvm-ar.exe"
            export RUSTUP_TOOLCHAIN="${rustToolchain}"
            export PATH="$cargo_home/bin:$PATH"
            exec "$cargo_home/bin/cargo" "$@"
          '';
          androidGradle = pkgs.writeShellScriptBin "tela-android-gradle" ''
            set -euo pipefail

            toolchain_root="''${TELA_ANDROID_TOOLCHAIN_ROOT:-''${XDG_CACHE_HOME:-$HOME/.cache}/tela/android}"
            gradle="''${TELA_ANDROID_GRADLE:-$toolchain_root/gradle/gradle-9.1.0/bin/gradle}"
            java_home="''${JAVA_HOME:-$toolchain_root/jdk}"
            if [ ! -x "$java_home/bin/java" ]; then
              printf '%s\n' 'Linux JDK 17 is absent. Run: nix develop .#android --command tela-android-bootstrap' >&2
              exit 1
            fi
            if [ ! -x "$gradle" ]; then
              printf '%s\n' 'Gradle 9.1 is absent. Run: nix develop .#android --command tela-android-bootstrap' >&2
              exit 1
            fi
            if [ ! -x "''${ANDROID_HOME:-}/build-tools/36.0.0/aapt2" ]; then
              printf '%s\n' 'Linux Android build-tools 36.0.0 are absent. Run: nix develop .#android --command tela-android-bootstrap' >&2
              exit 1
            fi
            export JAVA_HOME="$java_home"
            exec "$gradle" "$@"
          '';
          # iOS must use a complete local Xcode, not the Nix Apple SDK selected by the ordinary
          # macOS shell. Rustup and Cargo remain project-private so this target does not mutate a
          # developer's global Rust installation.
          iosBootstrap = pkgs.writeShellScriptBin "tela-ios-bootstrap" ''
            set -euo pipefail

            toolchain_root="''${TELA_IOS_TOOLCHAIN_ROOT:-''${XDG_CACHE_HOME:-$HOME/.cache}/tela/ios}"
            rustup_home="$toolchain_root/rustup"
            cargo_home="$toolchain_root/cargo"
            developer_dir="''${TELA_IOS_DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}"
            # Xcode 26 removed the in-bundle xcrun; fall back to the system shim.
            if [ -x "$developer_dir/usr/bin/xcrun" ]; then
              xcrun="$developer_dir/usr/bin/xcrun"
            else
              xcrun=/usr/bin/xcrun
            fi
            # The nix dev shell points DEVELOPER_DIR/SDKROOT at Nix's macOS SDK; force Xcode.
            export DEVELOPER_DIR="$developer_dir"
            unset SDKROOT

            if [ ! -x "$developer_dir/usr/bin/xcodebuild" ] || [ ! -x "$xcrun" ]; then
              printf '%s\n' 'Complete Xcode was not found. Install Xcode, launch it once, then set TELA_IOS_DEVELOPER_DIR if it is not /Applications/Xcode.app/Contents/Developer.' >&2
              exit 1
            fi
            if ! "$xcrun" --sdk iphoneos --show-sdk-path >/dev/null; then
              printf '%s\n' 'The selected Xcode has no iPhoneOS SDK. Complete Xcode (not Command Line Tools) is required.' >&2
              exit 1
            fi

            mkdir -p "$toolchain_root"
            # Rustup distribution mirror. TELA_IOS_RUSTUP_MIRROR is the base serving BOTH the
            # channel manifests at <base>/dist/ and the installer at <base>/rustup/dist/; TUNA
            # only mirrors dated cargo tarballs, so it does not qualify. Verified mirrors:
            #   USTC: TELA_IOS_RUSTUP_MIRROR=https://mirrors.ustc.edu.cn/rust-static
            #   SJTU: TELA_IOS_RUSTUP_MIRROR=https://mirror.sjtu.edu.cn/rust-static
            # Default is the official source. UPDATE_ROOT resolves to <base>/rustup, which the
            # mirrors above also serve.
            rustup_mirror="''${TELA_IOS_RUSTUP_MIRROR:-https://static.rust-lang.org}"
            rustup_env=(RUSTUP_DIST_SERVER="$rustup_mirror" RUSTUP_UPDATE_ROOT="$rustup_mirror/rustup")
            if [ ! -x "$cargo_home/bin/rustup" ]; then
              installer="$toolchain_root/rustup-init"
              printf '%s\n' "Downloading the project-local Apple Silicon Rust toolchain (''${rustup_mirror})..."
              curl --fail --location --retry 3 --retry-delay 2 \
                --output "$installer" \
                "$rustup_mirror/rustup/dist/aarch64-apple-darwin/rustup-init"
              chmod +x "$installer"
              RUSTUP_HOME="$rustup_home" \
                CARGO_HOME="$cargo_home" \
                env "''${rustup_env[@]}" \
                "$installer" -y --profile minimal --default-toolchain ${rustToolchain} --no-modify-path
            fi
            if ! RUSTUP_HOME="$rustup_home" CARGO_HOME="$cargo_home" \
              env "''${rustup_env[@]}" \
              "$cargo_home/bin/rustup" run ${rustToolchain} rustc --version >/dev/null 2>&1; then
              RUSTUP_HOME="$rustup_home" \
                CARGO_HOME="$cargo_home" \
                env "''${rustup_env[@]}" \
                "$cargo_home/bin/rustup" toolchain install ${rustToolchain} --profile minimal
            fi
            RUSTUP_HOME="$rustup_home" \
              CARGO_HOME="$cargo_home" \
              env "''${rustup_env[@]}" \
              "$cargo_home/bin/rustup" target add aarch64-apple-ios --toolchain ${rustToolchain}

            printf '%s\n' 'Tela iOS bootstrap complete.'
          '';
          iosCargo = pkgs.writeShellScriptBin "tela-ios-cargo" ''
            set -euo pipefail

            toolchain_root="''${TELA_IOS_TOOLCHAIN_ROOT:-''${XDG_CACHE_HOME:-$HOME/.cache}/tela/ios}"
            # Prefer the pinned toolchain from the user's rustup installation; the project-private
            # one installed by tela-ios-bootstrap remains an explicit fallback.
            if [ -n "''${TELA_IOS_CARGO_HOME:-}" ]; then
              cargo_home="$TELA_IOS_CARGO_HOME"
            elif global_cargo="$(command -v rustup >/dev/null 2>&1 && rustup which cargo --toolchain ${rustToolchain} 2>/dev/null)"; then
              cargo_home="$(dirname "$(dirname "$global_cargo")")"
            elif [ -x "$HOME/.rustup/toolchains/${rustToolchain}-aarch64-apple-darwin/bin/cargo" ]; then
              cargo_home="$HOME/.rustup/toolchains/${rustToolchain}-aarch64-apple-darwin"
            else
              cargo_home="$toolchain_root/cargo"
            fi
            developer_dir="''${TELA_IOS_DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}"
            if [ -x "$developer_dir/usr/bin/xcrun" ]; then
              xcrun="$developer_dir/usr/bin/xcrun"
            else
              xcrun=/usr/bin/xcrun
            fi
            if [ ! -x "$cargo_home/bin/cargo" ]; then
              printf '%s\n' 'Pinned iOS Rust toolchain is absent. Run: nix develop .#ios --command tela-ios-bootstrap' >&2
              exit 1
            fi
            if [ ! -x "$developer_dir/usr/bin/xcodebuild" ] || [ ! -x "$xcrun" ]; then
              printf '%s\n' 'Complete Xcode is required. Set TELA_IOS_DEVELOPER_DIR when Xcode is installed elsewhere.' >&2
              exit 1
            fi

            # The nix dev shell points DEVELOPER_DIR/SDKROOT at Nix's macOS SDK; force Xcode
            # before resolving the iPhoneOS SDK and toolchain paths.
            export DEVELOPER_DIR="$developer_dir"
            unset SDKROOT
            sdk_root="$("$xcrun" --sdk iphoneos --show-sdk-path)"
            clang="$("$xcrun" --sdk iphoneos --find clang)"
            ar="$("$xcrun" --sdk iphoneos --find ar)"
            export SDKROOT="$sdk_root"
            export IPHONEOS_DEPLOYMENT_TARGET=16.0
            unset MACOSX_DEPLOYMENT_TARGET
            export CC_aarch64_apple_ios="$clang"
            export CXX_aarch64_apple_ios="$clang++"
            export AR_aarch64_apple_ios="$ar"
            export CARGO_TARGET_AARCH64_APPLE_IOS_LINKER="$clang"
            # Rustup shims must select the release pinned by this flake rather than a moving
            # global stable channel. A leftover RUSTUP_HOME would redirect them to stale state.
            unset RUSTUP_HOME
            export RUSTUP_TOOLCHAIN="${rustToolchain}"
            export PATH="$cargo_home/bin:$PATH"
            exec "$cargo_home/bin/cargo" "$@"
          '';
          iosXcodebuild = pkgs.writeShellScriptBin "tela-ios-xcodebuild" ''
            set -euo pipefail

            developer_dir="''${TELA_IOS_DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}"
            xcodebuild="$developer_dir/usr/bin/xcodebuild"
            if [ ! -x "$xcodebuild" ]; then
              printf '%s\n' 'Complete Xcode is required. Set TELA_IOS_DEVELOPER_DIR when Xcode is installed elsewhere.' >&2
              exit 1
            fi
            export DEVELOPER_DIR="$developer_dir"
            unset SDKROOT
            unset MACOSX_DEPLOYMENT_TARGET
            # The nix shell exports LD/CC/LDFLAGS etc.; xcodebuild inherits them as build
            # settings and would link through a bare `ld` instead of the clang driver (breaking
            # its -Xlinker passthrough) and use the nix toolchain paths.
            unset LD CC CXX AR CPP LDFLAGS CFLAGS CXXFLAGS NIX_LDFLAGS NIX_CFLAGS_COMPILE NIX_CFLAGS_LINK
            exec "$xcodebuild" "$@"
          '';
          iosXcrun = pkgs.writeShellScriptBin "tela-ios-xcrun" ''
            set -euo pipefail

            developer_dir="''${TELA_IOS_DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}"
            if [ -x "$developer_dir/usr/bin/xcrun" ]; then
              xcrun="$developer_dir/usr/bin/xcrun"
            else
              xcrun=/usr/bin/xcrun
            fi
            if [ ! -x "$xcrun" ]; then
              printf '%s\n' 'Complete Xcode is required. Set TELA_IOS_DEVELOPER_DIR when Xcode is installed elsewhere.' >&2
              exit 1
            fi
            export DEVELOPER_DIR="$developer_dir"
            unset SDKROOT
            unset MACOSX_DEPLOYMENT_TARGET
            unset LD CC CXX AR CPP LDFLAGS CFLAGS CXXFLAGS
            exec "$xcrun" "$@"
          '';
          baseShellHook = ''
            # nix-direnv 会保留宿主 PATH 的顺序；显式前置项目级 ops，避免命中其他仓库的同名命令。
            export PATH="${opsCommand}/bin:$PATH"
            echo "tela product shell ready (cargo $(cargo --version | cut -d' ' -f2), node $(node --version))"
          '';
          coreShell = pkgs.mkShell {
            packages = corePackages;
            shellHook = baseShellHook + ''
              echo "tela core shell ready (pure Kernel + UI foundation)"
            '';
            RUST_BACKTRACE = "1";
            RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
          };
          webviewShell = pkgs.mkShell {
            packages = webviewPackages;
            shellHook = baseShellHook + ''
              echo "tela WebView shell ready (WASM + wasm-bindgen + pnpm)"
            '';
            RUST_BACKTRACE = "1";
            RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
          };
          assetsShell = pkgs.mkShell {
            packages = assetsPackages;
            shellHook = baseShellHook + ''
              echo "tela assets capability shell ready"
            '';
            RUST_BACKTRACE = "1";
          };
          unsupportedShell = product: pkgs.mkShell {
            packages = corePackages;
            shellHook = ''
              printf '%s\n' "tela ${product} is not supported on ${system}; select a supported product host." >&2
              exit 1
            '';
          };
        in {
          default = coreShell;
          core = coreShell;
          webview = webviewShell;
          assets = assetsShell;
          android = unsupportedShell "android";
          ios = unsupportedShell "ios";
          macos = unsupportedShell "macos";
          "render-wgpu" = unsupportedShell "render-wgpu";
          win32 = if pkgs.stdenv.isLinux then
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
              packages = webviewPackages ++ (with pkgs; [
                # Win32 开发壳：在 WSL 中交叉链接 x86_64-pc-windows-gnu 工件。
                pkgsCross.mingwW64.stdenv.cc
                cargoWin32
              ]);

              shellHook = baseShellHook + ''
                echo "tela Win32 shell ready (x86_64-pc-windows-gnu)"
              '';

              RUST_BACKTRACE = "1";
              RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
            }
          else unsupportedShell "win32";
        } // pkgs.lib.optionalAttrs pkgs.stdenv.isLinux {
          android = pkgs.mkShell {
            packages = webviewPackages ++ [
              androidBootstrap
              androidCargo
              androidGradle
            ];

            shellHook = baseShellHook + ''
              export TELA_ANDROID_TOOLCHAIN_ROOT="''${XDG_CACHE_HOME:-$HOME/.cache}/tela/android"
              export RUSTUP_HOME="$TELA_ANDROID_TOOLCHAIN_ROOT/rustup"
              export TELA_ANDROID_CARGO_HOME="$TELA_ANDROID_TOOLCHAIN_ROOT/cargo"
              if [ -x "$TELA_ANDROID_TOOLCHAIN_ROOT/jdk/bin/java" ]; then
                export JAVA_HOME="$TELA_ANDROID_TOOLCHAIN_ROOT/jdk"
                export PATH="$JAVA_HOME/bin:$PATH"
              fi

              android_windows_cmd="''${TELA_WINDOWS_CMD:-/mnt/c/Windows/System32/cmd.exe}"
              if [ -x "$android_windows_cmd" ]; then
                android_local_appdata="$("$android_windows_cmd" /D /S /C 'echo %LOCALAPPDATA%' 2>/dev/null | tr -d '\r')"
                if [ -z "''${TELA_ANDROID_WINDOWS_SDK_ROOT:-}" ] && [ -n "$android_local_appdata" ]; then
                  export TELA_ANDROID_WINDOWS_SDK_ROOT="$(wslpath -u "$android_local_appdata")/Android/Sdk"
                fi
                android_user_profile="$("$android_windows_cmd" /D /S /C 'echo %USERPROFILE%' 2>/dev/null | tr -d '\r')"
                if [ -z "''${TELA_ANDROID_GRADLE:-}" ] && [ -n "$android_user_profile" ]; then
                  android_gradle_cache="$(wslpath -u "$android_user_profile")/.gradle/wrapper/dists/gradle-9.1.0-bin"
                  if [ -d "$android_gradle_cache" ]; then
                    android_gradle_candidate="$(find "$android_gradle_cache" -type f -path '*/gradle-9.1.0/bin/gradle' -print -quit)"
                    if [ -n "$android_gradle_candidate" ]; then
                      export TELA_ANDROID_GRADLE="$android_gradle_candidate"
                    fi
                  fi
                fi
              fi
              export TELA_ANDROID_SDK_ROOT="$TELA_ANDROID_TOOLCHAIN_ROOT/sdk"
              export ANDROID_HOME="$TELA_ANDROID_SDK_ROOT"
              export ANDROID_SDK_ROOT="$ANDROID_HOME"
              if [ -n "''${TELA_ANDROID_WINDOWS_SDK_ROOT:-}" ]; then
                export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/27.1.12297006"
                export ANDROID_NDK_ROOT="$ANDROID_NDK_HOME"
                export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$TELA_ANDROID_WINDOWS_SDK_ROOT/ndk/27.1.12297006/toolchains/llvm/prebuilt/windows-x86_64/bin/aarch64-linux-android29-clang"
              fi
              echo "tela Android shell ready (arm64-v8a, API 29, Windows SDK / Linux build-tools 36)"
              if [ ! -x "$TELA_ANDROID_CARGO_HOME/bin/cargo" ] || [ ! -x "$TELA_ANDROID_TOOLCHAIN_ROOT/jdk/bin/java" ] || [ ! -x "$ANDROID_HOME/build-tools/36.0.0/aapt2" ]; then
                echo "Run once: tela-android-bootstrap"
              fi
            '';

            RUST_BACKTRACE = "1";
          };
          "render-wgpu" = pkgs.mkShell {
            packages = corePackages ++ (with pkgs; [ lld mesa vulkan-loader ]);
            shellHook = baseShellHook + ''
              export VK_DRIVER_FILES="${pkgs.mesa}/share/vulkan/icd.d/lvp_icd.x86_64.json"
              export LD_LIBRARY_PATH="${pkgs.vulkan-loader}/lib:''${LD_LIBRARY_PATH:-}"
              echo "tela WGPU renderer capability shell ready (lavapipe)"
            '';
            RUST_BACKTRACE = "1";
            RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
          };
        } // pkgs.lib.optionalAttrs pkgs.stdenv.isDarwin {
          macos = pkgs.mkShell {
            packages = webviewPackages;
            # AppKit/Metal 与 aarch64-apple-darwin stdlib/SDK 由本机 macOS 和 Xcode Command
            # Line Tools 提供；这里不携带 iPhoneOS/Xcode device 工具。
            shellHook = baseShellHook + ''
              echo "tela macOS shell ready (AppKit + Metal)"
            '';
            MACOSX_DEPLOYMENT_TARGET = "14.0";
            RUST_BACKTRACE = "1";
            RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
          };
          "render-wgpu" = pkgs.mkShell {
            packages = corePackages ++ [ pkgs.lld ];
            shellHook = baseShellHook + ''
              echo "tela WGPU renderer capability shell ready (Metal)"
            '';
            RUST_BACKTRACE = "1";
            RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
          };
          ios = pkgs.mkShell {
            packages = corePackages ++ [
              iosBootstrap
              iosCargo
              iosXcodebuild
              iosXcrun
            ];

            shellHook = baseShellHook + ''
              export TELA_IOS_TOOLCHAIN_ROOT="''${XDG_CACHE_HOME:-$HOME/.cache}/tela/ios"
              # The wrapper selects the flake-pinned rustup release; TELA_IOS_CARGO_HOME may point
              # at the project-private toolchain installed by tela-ios-bootstrap.
              export RUSTUP_HOME="''${RUSTUP_HOME:-$HOME/.rustup}"
              export TELA_IOS_CARGO_HOME="''${TELA_IOS_CARGO_HOME:-}"
              export TELA_IOS_DEVELOPER_DIR="''${TELA_IOS_DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}"
              # The default Darwin shell points at Nix's macOS SDK. This shell intentionally
              # delegates all device work to the wrappers above, which reset SDKROOT and select
              # the full Xcode iPhoneOS SDK for each command.
              export IPHONEOS_DEPLOYMENT_TARGET=16.0
              echo "tela iOS shell ready (iPhone arm64, iOS 16.0)"
              if [ ! -x "$TELA_IOS_CARGO_HOME/bin/cargo" ] && ! command -v rustup >/dev/null 2>&1; then
                echo "Run once: tela-ios-bootstrap"
              fi
              if [ ! -x "$TELA_IOS_DEVELOPER_DIR/usr/bin/xcodebuild" ]; then
                echo "Complete Xcode was not found at $TELA_IOS_DEVELOPER_DIR"
              fi
            '';

            RUST_BACKTRACE = "1";
          };
        }
        );
    };
}
