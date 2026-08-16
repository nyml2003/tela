# tela-ops — tela 开发运维工作流

> 把 tela 的日常开发操作（构建 / 验证 / 服务）收敛为一个 DDD 分层的 CLI。
> 运行时**零第三方依赖**（Node 内置 API + 本项目代码），工具链用最现代的 TS7。

## 为什么存在

旧开发方式是散装脚本：`scripts/check-architecture.sh`（bash 正则解析 Cargo.toml）、
`scripts/serve-demo.mjs`、手工三步构建（`cargo build --target wasm32-unknown-unknown`
→ `cp` → 外部冒烟脚本）。痛点：

- 构建 wasm 要手工三步，容易忘步骤；
- 验证门没有统一入口（fmt/clippy/test/arch 散在 flake 脚本里）；
- bash 解析 TOML 脆弱，改了依赖就崩。

## 入口

`ops` 是**项目级**开发命令，不提供用户级同名入口，避免一个仓库的工作流误指向另一个仓库。

1. **nix dev shell**（推荐）：执行 `direnv allow` 或 `nix develop` 后，flake 提供本项目的 `ops`
   可执行文件；在仓库根及任意子目录都可运行。

2. **临时调用**：在仓库根执行 `node ops/src/interface/cli.ts <命令>`。

`ops --help` / `ops help` 显示全部命令。

## Android 真机闭环

Android 只在显式进入 `nix develop .#android` 后构建。WSL 负责项目私有的 ARM64 Rust、Linux JDK、Linux
build-tools、bundle 和 Gradle；Windows Android Studio 提供 API 36 platform、NDK r27b、platform-tools、USB
授权、Logcat 与其自带的 `adb.exe`。真机不走局域网地址：`ops android serve`
固定监听 WSL 的 `127.0.0.1:8000`，`ops android deploy` 用 Windows ADB 建立 `adb reverse tcp:8000 tcp:8000`。

```bash
nix develop .#android --command tela-android-bootstrap # 首次；缓存 Rust/JDK/build-tools
nix develop .#android
ops build android
ops android serve
# 另一终端：nix develop .#android --command ops android deploy --serial <serial>
```

`TELA_WINDOWS_ADB` 可覆盖 Windows `adb.exe` 的 WSL 路径；未设置时，CLI 从 Windows
`%LOCALAPPDATA%/Android/Sdk/platform-tools/adb.exe` 自动发现。bootstrap 从 rsproxy 获取项目私有 Rust、从
TUNA 获取校验过的 Linux JDK，并直连校验固定 hash 的 Linux build-tools；它将 Windows SDK 的 platform、NDK、
platform-tools 和已接受许可证以链接接入项目缓存，不修改 Windows SDK、全局 Nix 或代理配置。

## 六条命令

| 命令 | 做什么 | 对应旧方式 |
|---|---|---|
| `ops check` | 四道验证门：fmt / clippy / test / **依赖方向检查**（TS 版，cargo metadata 真实依赖树，替代 bash 正则） | flake `check` + check-architecture.sh |
| `ops build [webview\|frontend\|bundle [desktop\|mobile]\|android\|win32\|macos\|all] [--release]` | `bundle` 可构建独立 desktop/mobile release Guest；`android` 先校验 mobile bundle，再构建 `arm64-v8a` Vulkan GameActivity APK，并固定写入 ADB-reverse localhost index；`webview`/`win32`/`macos` 保持各自壳职责，`all` 先重建目录 | 手工多步 |
| `ops verify [bundle [desktop\|mobile]\|gpu] [--build]` | 默认 desktop `bundle`：验证 `.tela` 的 archive/ABI/guest 首帧；可显式验证 mobile；`gpu`：服务原生 JS WebGPU 回读诊断页，不经过 tela renderer | 外部 smoke 脚本 |
| `ops serve [port]` | 开发静态服务器（默认 8000，端口占用自动递增，MIME/防穿越同旧脚本） | serve-demo.mjs |
| `ops android serve` | 只监听 `127.0.0.1:8000`；端口被占用立即失败，不自动改端口 | Android USB bundle 服务 |
| `ops android deploy [--serial SERIAL]` | 经 Windows `adb.exe` 验证单个 ARM64 设备、建立 reverse、安装并启动 APK | 手工 adb reverse/install/am start |

`ops build`（等同 `ops build all`）是默认 desktop/WebView 发布组合：它先重建 `dist/`，再生成 WebView
WGPU shell、浏览器 bundle 和 `tela-dev` 应用开发包。`tela-mobile` 是独立 channel，只有 `ops build bundle mobile`
或 `ops build android` 才会生成。Android command 不嵌入 guest archive，且通过 `tela-android-cargo` 使用 Rust
`aarch64-linux-android` target、Windows NDK r27b linker、API 36、Linux JDK 17 与 Gradle；它不会让日常浏览器构建被平台工具链阻塞。`ops build win32`
与 `ops build macos` 同样保持显式。单目标构建只更新自己拥有的工件，不清除其他输出。

## DDD 分层

```
src/
├── interface/      接口层：cli.ts（参数解析 + 依赖组装 + 分发，Node util.parseArgs）
├── application/    应用层：用例（check/build-webview/build-bundle/verify-bundle/serve/stress），只依赖端口
├── domain/         领域层：纯模型 + 端口（无 I/O，全部可单测）
│   ├── workspace.ts    路径模型（纯函数派生）
│   ├── gates.ts        验证门模型
│   ├── artifact.ts     构建工件模型
│   ├── architecture.ts 依赖方向规则（把 bash 脚本模型化为可测纯函数）
│   └── ports.ts        端口：Process/Fs/Server/Reporter
└── infrastructure/ 基础设施层：Node 适配器（子进程/fs/http/终端报告）
```

依赖方向：interface → application → domain ← infrastructure（端口实现注入应用层，
依赖倒置；domain 零 I/O 可离线单测）。

## 技术栈（为什么这样选）

- **Node 24 原生执行 TS**：`node src/interface/cli.ts` 直接跑，type stripping
  （要求 `erasableSyntaxOnly` 语法：无 enum / 参数属性 / namespace）；
- **TS7（native）typecheck**：`tsc --noEmit`（`typescript-native` 7.0.2）。
  typescript-eslint 8.66 尚不支持 TS7，按官方 side-by-side 方案与 TS 5.9.3 并存
  （pnpm 严格隔离：`.bin/tsc` → TS7，typescript-eslint peer → TS5.9）；
- **运行时零第三方依赖**：CLI 只用 Node 内置（child_process / http / fs / util）；
- 测试：`node --test`（内置 runner，直接跑 `.test.ts`）。

## 开发流程

```bash
pnpm install          # 安装 devDependencies（typescript/eslint/@types/node）
pnpm check            # typecheck (TS7) + lint + test（node:test）
pnpm dev -- stress    # 直接跑命令
```

## 与 tela 的边界

- ops 只碰**宿主侧开发流程**，不依赖 tela 的 Rust crate，也不被它们依赖；
- `ops check` 的依赖方向检查与旧 `scripts/check-architecture.sh` 规则等价
  （零依赖 crate / 白名单 / render 禁止反向依赖 core），数据源从 TOML 正则改为
  `cargo metadata --no-deps`（真实声明依赖，含 build/dev）；
