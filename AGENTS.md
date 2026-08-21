# AGENTS.md — tela 项目环境与 Nix 加速知识库

## Nix 下载慢的根因与修复（本项目实测，2026-08）

### 镜像优先级坑（最重要）
- Nix 按 `substituters` 配置顺序**固定**使用源：第一个能返回 narinfo 的源会被一直用，**不会自动选最快**。
- 国内直连镜像必须排在 `cache.nixos.org` **前面**，官方源放最后兜底。
- 当前生效顺序：`mirrors.ustc.edu.cn/nix-channels/store` → `mirror.sjtu.edu.cn/nix-channels/store` → `cache.nixos.org`。
- 实测延迟：cache.nixos.org ~216ms，USTC ~52ms，SJTU ~49ms，TUNA ~107ms。

### narinfo 探测法（判断镜像真可用，不是看 nix-cache-info）
- 只看 `nix-cache-info` 200 是**假象**（部分镜像 nar 实际 404）。
- **必须用纯 hash 形式**：`https://<mirror>/nix-channels/store/<hash>.narinfo`（`<hash>-<name>.narinfo`
  全名形式实测四源全 404，纯 hash 全 200——2026-08 实测确认，脚本已按此实现）。
- 注意：本机 flake 锁定的 nixpkgs rev（`531670d8…`）**不是官方 channel rev**（`02e08985a27c…`），
  其二进制在官方 cache 与镜像上全部 404（实测 0/23）→ 依赖全走本地编译。
  要命中官方预编译缓存，需把 rev 对齐官方 channel（`nix flake update nixpkgs` 后核对 `.git-revision`）。
- 一键检测脚本：`nix-mirror-check`（见下）。

### flake registry 与 tarball 下载（2026-08 实测）
- 用户级 registry 已改为 TUNA：`nix registry list` 应显示
  `user flake:nixpkgs https://mirrors.tuna.tsinghua.edu.cn/nix-channels/nixpkgs-unstable/nixexprs.tar.xz`
  （默认 global 是 channels.nixos.org，直连慢且易截断 "Truncated tar archive"）。
- **nix 下载 tarball 走 FlClash 代理会截断**（curl 走同一代理却成功）→ 必须 `no_proxy` 排除镜像域名
  （HM `home.sessionVariables.NO_PROXY` 已含 `mirrors.tuna.tsinghua.edu.cn,mirrors.ustc.edu.cn,mirror.sjtu.edu.cn,channels.nixos.org`；
  daemon 侧 `/etc/systemd/system/nix-daemon.service.d/override.conf` 需同步加）。
- 实测：no_proxy 直连 TUNA 40MB tarball 4.5MB/s 无截断；改后 `nix build nixpkgs#nodejs`（35.4MB 闭包）总耗时 1.45s。
- channel tarball 顶层带版本目录，但 nix 2.34 实测**可**直接作 flake 输入（自动下钻单顶层目录），
  早期 path: 输入测到 "flake.nix does not exist" 是 path 输入不下钻所致，tarball URL 输入正常。

### 配置层级与生效（踩过的坑）
- 配置优先级：命令行 > `~/.config/nix/nix.conf`（Home Manager 生成）> `/etc/nix/nix.conf`。
- **用户级配置覆盖系统级**：只改 `/etc/nix/nix.conf` 不会生效，必须同步改 HM 源头
  `~/.config/home-manager/home.nix` 的 `nix.settings.substituters`，再 `home-manager switch`。
- 改 `/etc/nix/nix.conf` 后必须重启 daemon：`sudo systemctl restart nix-daemon`（Determinate Nix systemd）。
- 本机为 Determinate Nix 2.34.7，daemon drop-in 在 `/etc/systemd/system/nix-daemon.service.d/override.conf`。

### flake 输入（本项目实测）
- **channel tarball 不能直接作为 flake 输入**：`channels.nixos.org` 与 TUNA/USTC 的 `nixexprs.tar.xz`
  顶层都带版本目录（如 `nixpkgs-26.11pre…/`），nix 2.34 实测报 `flake.nix does not exist`。
- 本项目保持 `inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05"`（tarball 已缓存，日常不进网络）。
- `nix build nixpkgs#<pkg>` 走 flake registry 默认源 = `channels.nixos.org/nixpkgs-unstable/nixexprs.tar.xz`，
  该源直连慢且易截断（实测 "Truncated tar archive"）→ 非必要少用裸 `nixpkgs#` 引用。
- system 必须为 `x86_64-linux`（WSL2 本机），`.envrc` 用 `use flake`，进入目录自动加载。

### 代理（WSL2 + FlClash）
- 本机为 **mirrored 网络模式**（Windows 侧 `.wslconfig` 配置）：WSL 内 `127.0.0.1` 直通 Windows，
  FlClash 需开 Allow LAN，混合端口 **7897**（非默认 7890）。
- 判定方法：WSL 内 `ss -tlnp | grep 789x` 无本机进程但 `curl 127.0.0.1:7897` 通 → mirrored。
- 代理变量由 Home Manager `home.sessionVariables` 固化（base.nix），daemon 侧在 override.conf。
- **镜像域名必须进 `no_proxy` 直连**：nix 经 FlClash 代理下载 tarball 会截断（curl 同代理却成功），
  直连国内镜像 4.5MB/s 无截断（HM 已配，daemon override.conf 需同步）。
- 若改用默认 NAT 模式：宿主 IP = `ip route show default | awk '{print $3}'`（形如 10.x.x.1），端口 7890。
- `NO_PROXY` 保留 `10.*,192.168.*,172.*,localhost` 等网段；国内镜像走代理实测 50ms 级，无需特判。

### Rust 加速
- `~/.cargo/config.toml` 已配置 rsproxy sparse 镜像（`sparse+https://rsproxy.cn/index/` +
  `git-fetch-with-cli = true`）。项目级 `.cargo/config.toml` 只含 wasm/Win32 链接器 flag，勿混。
- rustup 慢时临时用：`export RUSTUP_DIST_SERVER=https://rsproxy.cn/dist RUSTUP_UPDATE_ROOT=https://rsproxy.cn/rustup`
  （若 `rustup default stable` 报 "no release found"，重试官方源即可）。
- **绝不要手改 `~/.rustup/settings.toml`**（rustup 自管；改坏后删除该文件让 rustup 重建）。

## 防复发机制
- `~/.local/bin/nix-mirror-check`：真实 narinfo 检测 ustc/sjtu/tuna/official 四源；
  `--apply` 用 `NIX_SUDO_PASS` + `printf|sudo -S` 写 `/etc/nix/nix.conf` 并重启 daemon
  （配置内容先写临时文件再 `sudo cp`，避免 sudo -S 与 heredoc 抢 stdin）。
- 检测样本：`~/.local/share/nix-mirror-check/samples.txt`，每行一个 `<hash>` 或 `<hash>-<name>`
  （从真实下载日志 `nix build <pkg> -v 2>&1 | grep "copying path from"` 提取）。
- 改任何 nix 配置后验证：`nix config show | grep -A5 substituters`。

## macOS（Apple Silicon）开发机（2026-08 实测）

### 本机环境（与 WSL 机不同的地方）
- Apple M3 Pro，macOS 26.6，arm64；Determinate Nix 2.35.1。
- **Apple SDK 来自 nix**（非 Xcode CLT）：`xcode-select -p` 指向
  `/nix/store/…-apple-sdk-14.4`，dev shell 内 `SDKROOT`/`DEVELOPER_DIR` 已指到该 SDK，
  AppKit/Metal 链接走 nix clang 21 + cctools-binutils-darwin + xcbuild，无需装 Xcode。
- 真实 CLT 仍装在 `/Library/Developer/CommandLineTools`（swift/screencapture 等系统工具
  要用它：`DEVELOPER_DIR=/Library/Developer/CommandLineTools swift …`，且需 `env -u SDKROOT -u NIX_*`）。
- **wasm32 链接需要 `lld`**：nix rustc 不带 `rust-lld`，darwin dev shell 已加 `pkgs.lld`
  （提供 `wasm-ld`；Linux 壳本就带 lld）。缺它时 `ops build bundle` 报 `linker 'lld' not found`。

### macOS MVP 打通（024 文档验收通过）
- 流程：`ops build macos`（本机）→ `ops build bundle` + `ops serve 8001`（任意机器）
  → `./dist/macos/Tela.app/Contents/MacOS/tela-macos-sdk --bundle-index http://127.0.0.1:8001/tela-dev/latest.json --verbose`。
- `open Tela.app --args …` 也能起；直接跑二进制才有 stderr（verbose 指标）。
- 首次图形验证无辅助功能权限时：`screencapture` 与 `osascript` 拿不到窗口，可用
  `swift` + `CGWindowListCopyWindowInfo` 查窗口；App 侧 `--verbose` 打印 bundle/cache/guest 指标。

### objc2 0.6.4 宏语法（首次编译修错记录，已在源码中修正）
- `declare_class!` 已改名 `define_class!`；**不要再写 `unsafe impl ClassType` 与
  `impl DeclaredClass`**——超类走 `#[unsafe(super(NSView, NSResponder, NSObject))]`，
  ivars 走 `#[ivars = TelaViewIvars]`，线程走 `#[thread_kind = MainThreadOnly]`（需导入 `MainThreadOnly`）。
- 自定义方法用 `impl TelaView { #[unsafe(method(sel:))] … }`（无 `unsafe impl`）；
  `msg_send![super(self), setFrameSize: size]` 要 `let _: () =` 标注返回类型。
- 部分 AppKit API 在 feature 门后：`initWithContentRect_styleMask_backing_defer`、
  `NSBackingStoreType` 需 `NSGraphics`；`scrollingDeltaX/Y`、`NSFont::systemFontOfSize`
  需 `objc2-core-foundation`（tela-macos-sdk/Cargo.toml 已开）。
- `tela-render-wgpu` 离屏回读测试：适配器请求**不要** `force_fallback_adapter: true`
  （Metal 无 fallback → NotFound；去掉后 Linux lavapipe 仍会被枚举）。

### iOS 真机打通（028 验收通过，2026-08 实测，Xcode 26.6）
- **工具链走全局 rustup**（nix 提供的 rustup 1.29 + `~/.rustup` stable 1.97.1）：
  `rustup target add aarch64-apple-ios`（用 USTC 镜像：`RUSTUP_DIST_SERVER=https://mirrors.ustc.edu.cn/rust-static
  RUSTUP_UPDATE_ROOT=https://mirrors.ustc.edu.cn/rust-static/rustup`）。`tela-ios-cargo` 已改为全局优先
  （`TELA_IOS_CARGO_HOME` 可覆盖；shellHook 不再硬编码私有目录）。私有 `tela-ios-bootstrap` 保留为可选。
- **rustup 镜像布局**：变量指向 **base**（如 `mirrors.ustc.edu.cn/rust-static`）——channel manifest 在
  `<base>/dist/`，rustup-init 在 `<base>/rustup/dist/<triple>/rustup-init`，UPDATE_ROOT 是 `<base>/rustup/`。
  TUNA **不能**当 rustup 镜像（只有日期目录里的 cargo 包，无 rustup-init，实测 404）。
- **Xcode 26 三坑**（均已修进源码）：
  1. `-target` 与 `-derivedDataPath` 组合被禁 → xcodebuild 必须用 `-scheme TelaMobile`（ops 已改）。
  2. nix dev shell 导出 `LD=ld`/`CC=clang` 会污染 xcodebuild：链接变裸 `ld` 直接调用，`-Xlinker`
     参数解析失败（"unknown options: -Xlinker"）→ wrapper 必须 `unset LD CC CXX AR CPP LDFLAGS CFLAGS CXXFLAGS`。
  3. 工程引用的静态库文件被转成 `-l` 但**不带搜索路径**（`ld: library 'tela_ios_sdk' not found`）→
     pbxproj 需加 `LIBRARY_SEARCH_PATHS = $(SRCROOT)/build/rust`。
  - 另：Xcode 26 的 bundle 内已无 `xcrun`（只剩 `/usr/bin/xcrun`），wrapper 需回退（已修）。
- **真机部署前置（每个新设备都过一遍）**：
  1. iPhone 必须开 **Developer Mode**（设置→隐私与安全性→Developer Mode，开启会重启）——
     不开时 Xcode 无法把 UDID 注册进 Team，签名报 "Your team has no devices"。
  2. 签名用**具体设备 destination**：`-destination platform=iOS,id=<UDID>`（`generic/platform=iOS`
     报 "no devices"）；`-allowProvisioningUpdates` 让 Xcode 自动注册设备+生成 profile。
  3. 首次安装后 iPhone 需手动信任开发者证书（设置→通用→VPN与设备管理→信任），否则
     devicectl launch 报 Security/FBSOpenApplicationServiceError。
  4. 命令行注册设备不可靠时，Xcode GUI 里 ⌘R（scheme=TelaMobile，设备=手机）会走完
    「注册设备→生成 profile→安装启动」全链路，之后 `ops ios deploy` 可直接复用 profile。
- 本机设备：`风唤长河`（iPhone 17, iPhone18,3），UDID `31D43215-027C-5D5E-8558-235CD7D7C352`。
- 部署命令：`nix develop .#ios --command ops build ios` → `ops ios deploy --device <UDID>`。

## Win32 开发工作流约定（2026-08 起强制）

- **写完/改完任何 Win32 相关代码后，必须跑 release 交叉构建验证**：native `cargo check`
  不会编译 `cfg(target_os = "windows")` 代码（window.rs/gpu.rs/providers.rs 全部被跳过），
  只有交叉构建能抓出真实错误（曾漏掉 `IDC_HAND` 未导入、`SetCursor` 类型不匹配等）。
- 命令：`nix develop .#win32 --command ops build win32-editor --release`
  （`cargo-win32` wrapper 由 `.#win32` dev shell 提供，普通 `nix develop .` 没有）。
- 构建成功产出 `dist/win32-editor/tela-win32-editor-host.exe`，每次交付前以此为准。
- Windows 侧实测从 cmd 运行：`\\wsl$\Ubuntu\home\nyml\projects\tela\dist\win32-editor\tela-win32-editor-host.exe 2>log.txt`。
