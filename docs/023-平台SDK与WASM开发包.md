# 023-平台 SDK 与 WASM 开发包

> **状态：🟡 Win32、macOS、WebView 开发态已落地；Android source、mobile bundle、ARM64 host build workflow 与 Windows ADB deploy workflow 已落地，实际 ARM64 真机图形验收仍待执行。** 本文定义平台 SDK 的最小边界、可验证 WASM 开发包和后续平台的扩展方式；它不承诺生产分发方案。

## 1. 结论

平台不通过一个巨型 `Host` trait 接入，也不让 Win32 壳动态加载 Rust C ABI DLL。应用是可替换的 WASM guest，壳拥有窗口、渲染、输入和将来的系统桥；两者只交换版本化字节包，不交换 Rust 引用、trait object、native window handle 或业务 Store。

```text
tela-demo 或 tela-mobile-demo 应用源码
  -> app-wasm（release）
  -> tela-bundle (.tela + latest.json)
  -> 平台 SDK 启动时请求 archive
  -> SHA-256 / ABI / archive 条目校验
  -> Wasmtime guest（桌面/Android）或浏览器 WebAssembly guest（WebView）
  -> UiFrame
  -> tela-render-wgpu -> HWND / NSView / HTMLCanvasElement
```

桌面原生开发态每次**进程启动**请求一次包。没有热更新、轮询、跨版本状态迁移、后台预取或运行期二次下载；请求失败时仅回退到本机最后一个仍通过校验的包。窗口先显示原生 loading 页，随后由一次启动 worker 执行请求和 Wasmtime 初始化，UI 线程不会因网络或 guest 编译失去 native event loop。

通用 WebView 每次**页面启动**也请求一次包，但使用浏览器网络策略：`latest.json` 与 archive 一律 `cache: "no-store"`，当前不写 CacheStorage 或 IndexedDB，也没有浏览器本地 archive 回退。开发时选择可预期的新包优先于离线可用性；原生壳的最后有效包缓存不外溢到浏览器。

Android 每次**Activity 启动**严格请求当前 bundle：索引、archive、大小、SHA-256、ABI 或 Wasmtime
初始化任一步失败都显示原生诊断页，绝不读取桌面 cache，也不提供 Android cache fallback。这是 Android
选择共享 `tela-guest-runtime` 的原因，而不是共享桌面 GUI 生命周期的理由。

## 2. 为什么现在不是 C ABI 动态库

Win32 壳加载 C ABI DLL 在生产安装器、插件 ABI 或第三方语言嵌入时可以成立，但当前会过早引入 DLL 搜索路径、Rust allocator/恐慌边界、ABI 协商、文件锁和 WSL/Windows 工件同步。WASM 已经提供线性内存、显式 export、可检查版本号和跨平台执行面，足以满足“本地壳启动后取一次应用资源”的目标。

这不否定 C ABI：未来若 C/C++/Swift/Java 需要调用系统桥，可以另行导出小而稳定的 bridge ABI；它不替代应用包协议，也不进入 `tela-core`。

## 3. crate 与目录所有权

| crate / 目录 | 拥有内容 | 不拥有内容 |
| --- | --- | --- |
| `tela-app-abi` | `AppEvent`、`AppStatus`、帧包编解码、ABI 版本 | 窗口、renderer、业务副作用 |
| `tela-bundle` | `.tela` archive、内部清单、SHA-256、路径与条目验证 | HTTP、平台缓存、窗口 |
| `tela-demo` / `tela-mobile-demo` 的 `app-wasm` feature | 可移植 guest exports、各自应用状态、DSL 与 presentation | DOM、native window、GPU API、彼此的业务 model |
| `tela-guest-runtime` | Wasmtime guest、无窗口 verifier、严格 index/archive 校验 | GUI 生命周期、window/surface、IME、缓存、任何 Target SDK |
| `tela-webview-sdk` | 浏览器壳的 Rust 部分：bundle/ABI 校验、事件编解码、`UiFrame` 解码、WGPU surface/present | DOM 事件循环、HTTP fetch、浏览器缓存策略、Android/iOS bridge |
| `web/src/webview-sdk/` | DOM 输入/IME、viewport/DPR、fetch、浏览器 guest 实例化、会话关闭 | bundle 格式解释、帧协议编码、渲染命令生成 |
| `tela-native-sdk-runtime` | 桌面 HTTP loader、最后有效缓存和 desktop shell lifecycle | Wasmtime 核心执行、Android 生命周期、窗口、WGPU surface、平台输入 |
| `tela-win32-sdk` | HWND/WGPU、Win32 输入归一化和 Windows 启动 worker | `tela-core` 内部实现、业务逻辑 |
| `tela-macos-sdk` | AppKit/NSView、Metal/WGPU、macOS 输入和缓存启动 worker | Win32 消息、业务逻辑 |
| `tela-android-sdk` + `android/` | GameActivity、Winit、Vulkan/WGPU、触摸、JNI whole-value IME、system Back、APK 打包 | desktop cache/lifecycle、业务 guest、GLES fallback |
| `ops` | 构建、发布顺序、bundle 验证和静态服务 | runtime 协议解释、平台窗口 |

`tela-render-wgpu` 仍只消费 `UiFrame`。浏览器、游戏与原生壳不各自复制布局或绘制命令生成逻辑。`web/src/webview-sdk/` 是 Rust crate 的 JavaScript 半边，不是第二个 renderer；它让未来 Android/iOS WebView 能复用通用 DOM 壳，而不提前发明 native bridge。

## 4. 开发包与启动协议

`ops build bundle` 与 `ops build bundle mobile` 都将各自 Guest 的 `app-wasm` 编译为 **release WASM**，
生成两个彼此独立的 delivery channel：

```text
dist/tela-dev/
├── latest.json       小索引：format、ABI、archive URL、大小、archive SHA-256
└── tela-demo.tela    zip archive：app.wasm、assets/、manifest.json

dist/tela-mobile/
├── latest.json       mobile Guest 的独立 index
└── tela-mobile-demo.tela
```

archive 内 `manifest.json` 声明 `app.wasm` 与每个资源条目的未压缩长度、SHA-256 和相对路径；内部 `bundle_id` 由有序声明重新推导。路径只能是 `app.wasm` 或 `assets/` 下的普通相对路径，拒绝绝对路径、`..` 和反斜杠；archive 限制单条 64 MiB、总解压 256 MiB、最多 1024 个资源。

原生壳通过 Wasmtime 执行 guest，因此 debug WASM 会使普通文字度量与布局消耗过多 fuel，且可能将 resize 误判为 guest 失效。WebView 用浏览器原生 WebAssembly 实例化同一个 release guest，不引入 Wasmtime 或 fuel 模拟。两类执行器都必须在启动时重新校验网络边界，不能因为构建端曾校验就信任下载内容。

`assets/` 会被校验并随原生包缓存，但 guest 到原生 renderer 的资源解析/GPU 上传通道尚未接入；示例不会因 archive 内有图片就自动生成 `Image`。这是后续资源协议，不是 SDK 的既有能力。

### 4.1 桌面原生启动

1. UI 线程创建 HWND/NSWindow，立即显示 loading，并继续处理关闭、焦点、尺寸与 DPI。
2. 启动 worker 读取 `latest.json`，检查 format、ABI、大小和 archive URL。
3. worker 下载 archive，核对压缩包大小/哈希，并检查内部 ABI、条目路径、长度和每条哈希。
4. 成功后原子写入最后有效缓存；网络或校验失败时以同一套检查尝试缓存。
5. worker 编译/实例化 guest、验证首个 frame/status，经队列通知 UI 线程。
6. UI 线程创建 WGPU surface/device，派发首个 viewport，在本线程解码 `UiFrame` 并绘制。
7. worker 和缓存均失败时，loading 切换到原生错误页；不会启动半成品 guest 或 GPU。

`GuestRuntime` 保留已验证的 frame bytes，而不把 `UiFrame` 伪装为可跨线程对象。缓存只服务开发体验，删除它只会失去离线回退，不影响源码或 `dist/`。

### 4.2 Android 启动

1. Kotlin `TelaActivity` 在 `GameActivity` 初始化前把 Gradle 注入的 `telaBundleIndex` 交给 Rust；空 URL
   只显示诊断页，不能启动旧 Guest。
2. `android_main` 创建 Winit event loop；后台 worker 使用 `tela-guest-runtime` 请求并校验 index/archive，
   再以 Wasmtime 实例化 Guest。
3. `resumed` 时 Android host 创建窗口、Vulkan-only WGPU instance/surface/device/renderer；`suspended` 时先
   drop surface 与 window，但保留已验证的 portable Guest。
4. Guest ready 后主 event loop 派发逻辑 viewport，读取 `UiFrame` 并绘制；任何 bundle 或 Guest 失败都
   留在原生错误诊断，不触发 cache fallback。

Android 与桌面共享的只有 app ABI、bundle 校验和 `GuestRuntime`。GameActivity、surface 生命周期、触摸、
IME 和 Back 都是 Android 专属实现，不能迁回 shared runtime。

### 4.3 WebView 启动

1. `startTelaWebview({ canvas, bundleIndex })` 检查浏览器 WebGPU 能力并加载固定的 `tela_webview_sdk.js` / `_bg.wasm`。
2. JavaScript 以 `no-store` fetch 索引和 archive；相对 archive URL 由浏览器 `URL` API 解析。
3. Rust SDK 校验 index、大小、SHA-256、内部 archive、ABI 和路径，取得 `app.wasm`。
4. JavaScript 实例化 guest，读取 ABI、首帧和 status；线性内存读取均做长度/范围上限检查。
5. Rust SDK 创建 canvas WGPU surface；DPR backing store 同步后，向 guest 派发逻辑 viewport、安装输入桥并请求首帧。

默认 index 为相对当前页面解析的 `/tela-dev/latest.json`。开发服务器仅为 `/tela-dev/*` 添加 `Access-Control-Allow-Origin: *`，以支持明确的跨机器开发；其他静态资源不开放跨域读取。

### 4.4 构建端发布门

两个 bundle command 都先写 channel 内的 `.tmp` archive 与 `.tmp` index，随后执行中性的
`tela-guest-verify <tmp>`。验证器检查 archive、以 Wasmtime 实例化 guest、读取首帧并派发一次 viewport；
只有通过后才依次替换 archive 与索引。验证失败时运行中的开发服务器仍可提供该 channel 的上一份完整包。

## 5. WASM 应用 ABI

guest 通过线性内存导出下列 C ABI 形状的函数。它们是 WASM export，不是 native DLL ABI：

| export | 作用 |
| --- | --- |
| `tela_app_abi_version() -> u32` | 必须等于 host 的 ABI 版本 |
| `tela_app_init() -> u32` | 重置应用并发布首帧 |
| `tela_app_input_begin(bytes) -> ptr` | 保留输入包线性内存 |
| `tela_app_dispatch(bytes) -> u32` | 消费一个 `AppEvent` 并发布新帧/状态 |
| `tela_app_frame_ptr/len` | 当前编码 `UiFrame` 的线性内存范围 |
| `tela_app_status_ptr/len` | 当前 `AppStatus` 的线性内存范围 |
| `tela_app_error_ptr/len` | 最近失败的 UTF-8 诊断 |

SDK 每次 dispatch 都重新读取完整帧和状态，避免 host 保留 guest 内存借用。包包含 magic 与 ABI 版本；超过 64 MiB 的输入或输出会被拒绝。wire frame 仅允许已有跨后端绘制原语，`DrawPayload::Custom` 不能跨此边界。

当前 ABI 为 **v2**。v2 保持既有 packet header 与旧事件编码顺序，在尾部追加 `InputCompositionStart`、`InputCompositionEnd` 与 `ReplaceKeymapJson(String)`：composition 只标记 IME 交互生命周期，受控文本仍由 `SetInputValue` 传递；键位表替换由 guest 原子校验。壳始终传递物理键与 modifier bits，键盘意图只在应用层解析。

## 6. 输入、焦点与生命周期

### 6.1 桌面原生壳状态机

`tela-native-sdk-runtime` 的无窗口 `ShellLifecycle` 管理 `Loading`、`Running`、`Suspended`、`Failed` 和 `Closing`。HWND/NSView、WGPU instance/surface/device/renderer、解码后的 `UiFrame` 与 guest dispatch 均由对应 UI 线程拥有；启动 worker 只构造 loader 与 `GuestRuntime`，经队列交接。

`request_redraw` 在状态机内去重；多个输入或 resize 只产生一个待处理 redraw。surface timeout 使用 `16/32/64/128/250ms` 的单一短 retry，成功 present 后清零。Win32 surface 必须以匹配的 display owner 与 HWND 创建；`Outdated`/`Lost` 重新 configure，`Occluded` 跳过本帧，`Validation` 记录诊断并有序关闭。

原生壳把 DPI 逻辑坐标、滚轮、物理键、修饰键和基础文本归一化为 `AppEvent`。文本焦点由 `AppStatus.input_focused` 与原生窗口焦点共同决定；进入/离开各补发一次 `InputFocus`/`InputBlur`，避免 Tab 切换留下草稿或状态栏提示。当前原生范围不包括完整中文 IME、死键、剪贴板、拖放、无障碍树和系统桥；后续 IME 必须作为 `tela-widgets` 文本原子的专门 native text channel 接入。

### 6.2 WebView session

```ts
const session = await startTelaWebview({
  canvas,
  bundleIndex: new URL('/tela-dev/latest.json', window.location.href),
});

session.replaceKeymap({ /* 已知 schema 的键位表快照 */ });
session.close();
```

每个 session 独占一个可聚焦 `HTMLCanvasElement`、隐藏受控 `<textarea>` 和一套 Rust WGPU session。启动若任一步失败，会反向释放已经取得的资源；关闭时取消动画帧/短 retry、`ResizeObserver`、DPR media query、DOM listener、隐藏编辑器和 WGPU session，重复 `close()` 无副作用。

DOM 适配仅做平台归一化：pointer/wheel 按当前逻辑 viewport 映射，`KeyboardEvent.code` 映射 USB-HID physical key，modifier 保持 bit mask。textarea 产生受控文本、focus/blur 与 composition start/end；guest 的 `AppStatus` 决定 textarea 是否持有焦点和 canvas cursor。`replaceKeymap` 只是 ABI action，不替代应用自身的键位表校验。

WebView 产品路径不使用 raster 回退。surface 的 `Outdated`、`Lost`、`Timeout`、`Occluded` 由 session 合并重绘并短 retry；不可恢复 WGPU 问题记录到控制台并带 Rust 诊断。`rawgpu.html` 是单独的浏览器环境检查，不构成应用回退。

### 6.3 Android Target

Android 使用 `GameActivity`，而不是把现有 Win32/macOS 壳套进 `NativeActivity`。Winit 的 Android GameActivity
API 与 Cargo 的 `android-game-activity` feature 必须来自同一版本；Vulkan-only `wgpu::Backends::VULKAN` 是
首期硬约束，不存在 GLES 回退。构建只选 `arm64-v8a`，`minSdk = 29`，`compileSdk/targetSdk = 36`。

触摸适配器只捕获首个 pointer。触点在 touch slop 内直到 release 才发 `PointerDown` + `PointerUp`，越过
slop 后只发反向内容 delta 的 `PointerScroll`；没有惯性、pinch 或多指语义。Kotlin 隐藏 `EditText` 以
whole-value 受控同步 `AppStatus.input_value` 和 `SetInputValue`，以 composition start/end 标记 IME 段落，
光标固定在末尾。首期没有选择、复制粘贴、候选框坐标或多段编辑。

系统 Back 的次序固定：文本输入活动时先 blur/hide IME；否则向 Guest 派发 Escape。mobile Guest 依次处理
预览返回、文件夹返回、清空查询和根目录未处理；只有最后一种情况 Kotlin 才 finish Activity。

## 7. 开发命令

在 WSL 仓库根执行：

```bash
ops build
ops verify bundle
ops build bundle mobile
ops verify bundle mobile
ops serve
```

原生壳可单独构建：

```bash
ops build win32
ops build macos # 仅 Apple Silicon macOS
nix develop .#android --command tela-android-bootstrap
nix develop .#android --command ops build android
nix develop .#android --command ops android serve
nix develop .#android --command ops android deploy --serial <serial>
```

Windows 默认请求 `http://127.0.0.1:8000/tela-dev/latest.json`；`--port` 只替换本机端口，`--bundle-index` 允许完整 HTTP(S) 地址，两者互斥。Mac 或局域网开发应使用开发机可达地址和显式 `--bundle-index`。Android 不再接受可变 URL：其 APK 固定请求 `http://127.0.0.1:8000/tela-mobile/latest.json`，`ops android deploy` 以 Windows `adb.exe` 建立 USB `adb reverse tcp:8000 tcp:8000`，所以真机看到的是自己的 localhost。首次运行的 bootstrap 在项目缓存准备 Rust `aarch64-linux-android` target、Linux JDK 17 与 Linux build-tools，并只读链接 Windows API 36、NDK r27b、platform-tools 和许可证；`ops build android` 不依赖 `cargo-ndk`。`ops serve` 只服务 `dist/`，不会启动浏览器。

## 8. 后续平台

| 平台 | 壳职责 | 应用协议 | 当前状态 |
| --- | --- | --- | --- |
| Win32 | HWND、WGPU、Win32 输入、缓存 | `tela-app-abi` + `.tela` | 开发态已实现 |
| macOS | AppKit/NSView、Metal WGPU surface、输入、缓存 | 同一 ABI/bundle | Apple Silicon 开发态已实现；真机图形验收见 024 |
| WebView | DOM canvas、WGPU、DOM 输入/IME、一次性网络加载 | 同一 ABI/bundle；浏览器 WebAssembly 执行 guest | 通用开发态已实现；不含 Android/iOS native bridge |
| Android | GameActivity、Vulkan/WGPU、触摸、IME、Back、surface 生命周期 | 同一 ABI/bundle；严格远程 Wasmtime guest | ARM64 cross-APK 与 Windows ADB deploy 已实现；真机图形验收待执行 |
| iOS | UIKit/Metal layer、触摸、IME、生命周期 | 同一 ABI/bundle | 仅设计目标 |

Android 已在真实窗口、输入、发布与 bridge 约束出现后增加薄 SDK；iOS 仍不预建空 crate。后续 Target 只在真实需求出现时复用 `tela-app-abi`、`tela-bundle`、`tela-guest-runtime` 或 renderer 的已验证部分；不要为了名义统一先创造共享大接口。

## 9. 验收边界

- `ops build bundle` 与 `ops build bundle mobile` 能从干净的 `dist/` 产出各自 release guest 包，并在 Wasmtime 首帧/viewport 校验通过后最后发布各自索引。
- `ops build webview` 产出 wasm-bindgen WebView WGPU shell；`ops build` 将它、浏览器静态页面和同一开发包一起重建。
- `ops build win32` 交叉编译 GNU Windows 壳到 `dist/win32/`；`ops build macos` 在 Apple Silicon macOS 构建 `dist/macos/Tela.app`，App 只含本地壳。
- 自动化覆盖 ABI、SHA-256、路径、原生缓存回退、bundle guest 初始化、WebView bundle 验证和 WebView 构建用例；构建/验证不以启动浏览器代替测试。
- Windows 真机应显示完整 `UiFrame`，响应鼠标、键盘、Tab/方向焦点和失焦恢复，且 `--verbose` 不再出现缺少 display handle。macOS 真机应以显式 `--bundle-index` 验证 AppKit/Metal、resize、输入、cache fallback 和 display handle，详见 [024](024-macOS开发SDK实施目标.md)。
- 浏览器人工验收由开发者在自己选择的 WebView/浏览器打开 `ops serve` 输出根 URL，验证 WGPU、DPR resize、树/列表输入、Tab/方向键焦点、IME composition 与 `window.telaReplaceKeymap`；本仓库命令不会自行启动 Chromium。
- Android 自动化已覆盖独立 mobile bundle、strict verifier、触摸 slop、单指取消、whole-value IME、ARM64 构建、固定服务与 ADB deploy 失败闭环；完成项目 Android 工具链后，还必须执行 `ops build android`、`ops android serve`、`ops android deploy --serial ...` 并在 ARM64 真机验证 Activity resume/suspend、Vulkan surface、IME、Back 和远程 bundle 失败诊断。
