# 023-平台SDK与WASM开发包

> **状态：🔧 Win32 与 macOS 开发态已落地。** 本文定义平台 SDK 的最小边界、可验证 WASM 开发包和后续平台的扩展方式；它不承诺生产分发方案。

## 1. 结论

原生平台不通过一个巨型 `Host` trait 接入，也不在 v1 让 Win32 壳去动态加载 Rust C ABI DLL。当前实现采用更小、更可验证的边界：

```text
tela-demo 应用源码
  -> app-wasm 编译
  -> tela-bundle (.tela + latest.json)
  -> Win32 / macOS SDK 启动时请求一次
  -> SHA-256 / ABI / archive 条目校验
  -> Wasmtime guest
  -> UiFrame
  -> tela-render-wgpu -> HWND surface
```

壳是 Rust 编译出的原生 exe，拥有 Win32 SDK、WGPU surface、事件循环和将来系统桥的位置；应用是可替换的 WASM guest，拥有状态、DSL、布局、交互和 `UiFrame`。两者只交换版本化字节包，不交换 Rust 引用、trait object、HWND 或业务 Store。

开发态每次**进程启动**请求一次包。没有热更新、轮询、跨版本状态迁移、后台预取或运行期二次下载；请求失败时仅回退到本机最后一个仍然通过校验的包。窗口会先显示一个原生 loading 页面，随后仅由一次启动 worker 执行这次请求和 Wasmtime 初始化，UI 线程不会因为网络或 guest 编译失去 Win32 消息泵。

## 2. 为什么不是 C ABI 动态库

Win32 壳加载 C ABI DLL 在生产安装器、插件 ABI 或第三方语言嵌入时可以成立，但现在会过早引入：DLL 装载路径和依赖搜索、Rust allocator/恐慌边界、ABI 版本协商、增量替换时的文件锁，以及 Windows/WSL 开发机间的工件同步。WASM 已经提供受限线性内存、显式 export、可检查的版本号和跨平台执行面，足以解决当前“本地壳启动后取一次应用资源”的目标。

这不是否定 C ABI：未来若系统桥必须由 C/C++/Swift/Java 调用，桥层可以另行导出一组小而稳定的 C ABI；它不替代应用包协议，也不进入 `tela-core`。

## 3. crate 所有权

| crate / 目录 | 拥有内容 | 不拥有内容 |
| --- | --- | --- |
| `tela-app-abi` | `AppEvent`、`AppStatus`、帧包编解码、ABI 版本 | 窗口、renderer、业务副作用 |
| `tela-bundle` | `.tela` archive、内部清单、SHA-256、路径与条目验证 | HTTP、平台缓存、窗口 |
| `tela-demo` 的 `app-wasm` feature | 可移植 guest exports、应用状态和 DSL | DOM、HWND、GPU API |
| `tela-native-sdk-runtime` | HTTP bundle loader、最后有效缓存策略、Wasmtime guest、无窗口 verifier、共享生命周期与启动 CLI | 窗口、WGPU surface、平台输入 |
| `tela-win32-sdk` | HWND/WGPU、Win32 输入归一化和 Windows 启动 worker | `tela-core` 内部实现、业务逻辑 |
| `tela-macos-sdk` | AppKit/NSView、Metal/WGPU、macOS 输入和 `~/Library/Caches` 启动 worker | Win32 消息、业务逻辑 |
| `ops` | 构建、发布顺序和静态服务 | runtime 协议解释、平台窗口 |

`tela-render-wgpu` 仍只消费 `UiFrame`；浏览器、游戏与 Win32 不各自复制布局或绘制命令生成逻辑。

## 4. 开发包与启动协议

`ops build bundle` 总是将 `tela-demo` 的 `app-wasm` 编译为 **release WASM**，再产生两个文件：

```text
dist/tela-dev/
├── latest.json       小索引：format、ABI、archive URL、大小、archive SHA-256
└── tela-demo.tela    zip archive：app.wasm、assets/、manifest.json
```

archive 内的 `manifest.json` 再声明 `app.wasm` 和每个资源条目的未压缩长度、SHA-256 与相对路径；内部 `bundle_id` 由这些声明的有序内容重新推导。路径只能是 `app.wasm` 或 `assets/` 下的普通相对路径，拒绝绝对路径、`..` 和反斜杠路径；archive 还限制单条 64 MiB、总解压 256 MiB 和最多 1024 个资源。

这里刻意不复用浏览器开发页的 debug WASM。平台壳通过 Wasmtime 执行 guest；未优化 WASM 会让正常的文字度量和布局消耗数百倍 fuel，并把窗口 resize 误判成 guest 失效。开发期仍是“改源码后重新构建并启动时取一次包”，只是运行包本身必须经过优化。

v1 会把 `assets/` 一并校验并缓存，保证开发包的完整性；但 guest 到原生 renderer 的资源解析与 GPU 上传通道尚未接入。因此当前示例不会因为 archive 内有图片就自动把它绘制成 `Image`，这项能力属于后续资源协议，而不是本次 SDK 的既有功能。

启动顺序：

1. UI 线程创建并显示 HWND，立即画出原生 loading 页面，然后继续处理关闭、移动、焦点和 DPI 消息。
2. 启动 worker 读取 `latest.json`，检查格式版本、ABI、大小和 archive URL。
3. worker 下载一个 archive，核对压缩包大小与 SHA-256，并读取 archive 检查内部 ABI、条目路径、长度和每条内容哈希。
4. 成功后 worker 写入 `%LOCALAPPDATA%\\tela\\development\\last-valid.tela`；临时文件完成后再替换缓存。网络或校验失败时，worker 以同一套校验尝试最后有效缓存。
5. worker 编译/实例化 guest、调用初始化 export，并验证首个 frame/status 包；完成后只通过队列和 `WM_APP` 通知 UI 线程，绝不把 `WindowState` 指针交给后台线程。
6. UI 线程接收成功结果后创建 WGPU surface/device，派发首个 viewport，再在本线程解码 `UiFrame` 并绘制。`GuestRuntime` 保留已验证的 frame bytes 而不持有 `UiFrame`，因为后者允许 host-only `CustomDraw` trait object，不能被伪装成可跨线程对象。
7. worker 和缓存都不能给出有效包时，loading 页面切换为原生错误页；应用 guest、GPU 和半成品 `UiFrame` 都不会启动。

缓存只服务开发体验，不是持久化业务数据；删除它只会使离线启动失去回退，不影响源码或 `dist/`。

构建端也有同一条运行时门：`ops build bundle` 先写 `tela-demo.tela.tmp` 和 `latest.json.tmp`，然后执行 `tela-sdk-verify <tmp>`（由 `tela-native-sdk-runtime` 提供）。验证器会检查 archive、以 Wasmtime 实例化 guest、读取首帧，并派发一次 viewport 事件；只有全部通过才依次替换 archive 和索引。验证失败不会发布新索引，运行中的开发服务器仍可提供上一份完整包。

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

SDK 每次 dispatch 重新读取完整帧和状态，避免 host 持有 guest 内存借用。包包含魔数和 ABI 版本；超过 64 MiB 的输入或输出会被拒绝。当前 wire frame 只允许现有跨后端绘制原语，`DrawPayload::Custom` 不能跨此边界，防止把平台私有对象混进包协议。

## 6. 输入、焦点与生命周期

### 6.1 所有权与状态机

`tela-native-sdk-runtime` 以一个无窗口/WGPU 依赖的 `ShellLifecycle` 管理壳状态；其转换可在 Linux 单测，不依赖 Windows 或 macOS 真机。平台资源仍严格由各自 UI 线程拥有：HWND/NSView、WGPU instance/surface/device/renderer、解码后的 `UiFrame` 和所有 guest event dispatch 都不能离开消息线程。启动 worker 只拥有 loader 与 `GuestRuntime` 的构造过程，结果经 `mpsc` 队列交接；Win32 以标量 `WM_APP` 唤醒，macOS 由 AppKit run-loop timer 轮询已完成消息。

| 状态 | 进入条件 | 壳行为 | 合法退出 |
| --- | --- | --- | --- |
| `Loading` | HWND 创建完成 | 绘制原生 loading；接受关闭与焦点消息；后台拉包/编译 | worker 成功且 client 非零 -> `Running`；最小化 -> `Suspended`；失败 -> `Failed` |
| `Running` | guest、非零 client、GPU 均可用 | 消费输入、合并重绘、present `UiFrame` | 最小化/零尺寸 -> `Suspended`；关闭 -> `Closing` |
| `Suspended` | client 为零或窗口最小化 | 不向 guest 派发零 viewport，不 acquire surface；保留 guest，GPU 可在 device loss 后等待恢复 | client 恢复 -> `Running`；关闭 -> `Closing` |
| `Failed` | loader、guest 或首次 GPU 初始化失败 | 只画原生错误诊断，不运行半成品 guest | 用户关闭 -> `Closing` |
| `Closing` | `WM_CLOSE` 或不可恢复运行时错误 | 取消迟到 startup result、释放 capture/hover/text channel/timer，随后在 `WM_DESTROY` 丢弃 surface | 无 |

`request_redraw` 在状态机内去重；多个输入或 resize 只产生一个待处理的 `WM_PAINT`。surface timeout 采用 `16/32/64/128/250ms` 的单一临时 retry，成功 present 后清零，避免忙等和无限消息循环。

### 6.2 WGPU surface 与 device

Win32 的 `InstanceDescriptor` 显式持有 `WindowsDisplayHandle` 对应的 display owner，创建 surface 时也传入同一个 Windows raw display marker 和 HWND raw window handle。不能再把两者同时传 `None`：wgpu 30 会在创建 presentable surface 时拒绝缺少 display handle 的壳。

| `get_current_texture()` 结果 | Win32 壳处理 |
| --- | --- |
| `Success` | 绘制并 present，重置 timeout backoff |
| `Suboptimal` | 绘制并 present，然后重新 configure 并请求下一帧 |
| `Outdated` | 重新读取非零 client 尺寸并 configure，再请求下一帧 |
| `Lost` | 用同一 instance 重新创建 surface、configure，再请求下一帧 |
| `Timeout` | 不阻塞 UI；启动一个受状态机约束的短 timer 后重试 |
| `Occluded` | 跳过当前帧，等待 size/focus/下一次失效触发 |
| `Validation` | 写入明确诊断并有序关闭，避免假装绘制成功 |

device-lost callback 可能来自非 UI 线程，因此回调只记录诊断和 GPU generation，再投递标量 `WM_APP`。UI 线程忽略旧 generation 的迟到通知；当前 generation 的第一次 loss 会完整重建 instance/surface/device/renderer，第二次 loss 或重建失败会打印诊断并有序退出。若 loss 发生在 `Suspended`，旧 GPU 立即失效，重建延后到 client 恢复；不会把零尺寸传给 `Surface::configure`。

### 6.3 输入与焦点

Win32 壳把 HWND 消息转换为 `AppEvent`：指针按逻辑 DPI 坐标、滚轮、物理键、修饰键和基本文本。键盘意图与可运行时替换的键位表仍由应用层解析，壳只提供归一化 physical key 与 modifier bits。

文本焦点由 guest 的 `AppStatus.input_focused` 决定，native window focus 由 `WM_SETFOCUS`/`WM_KILLFOCUS` 决定；只有两者同时为真时，状态机才补发一次 `InputFocus`。离开输入、窗口失焦或关闭时只补发一次 `InputBlur`，随后重新读取 guest 状态，因此 Tab 在两个输入之间移动不会遗留旧草稿，也不会因为重复 blur 改写底部提示。

`WM_MOUSEMOVE` 注册一次性 `TrackMouseEvent(TME_LEAVE)`；`WM_MOUSELEAVE`、失焦和关闭都会向 guest 发送 `PointerMove(-1, -1)` 来清掉 hover。主键按下设置 native capture；`WM_LBUTTONUP`、`WM_CAPTURECHANGED`、`WM_CANCELMODE` 和关闭统一清理本地 capture，不虚构未定义的 `PointerCancel` ABI。

当前范围：鼠标、滚轮、Tab/方向键/快捷键、回车/取消、ASCII 文本。中文 IME、死键、剪贴板、拖放、无障碍树和系统桥尚未实现，不能把 `WM_CHAR` 当作完整文本输入方案。后续 IME 应作为 `tela-widgets` 文本原子的专门 native text channel 接入，而不是在当前 `WM_CHAR` 分支堆补丁。

## 7. 开发命令

在 WSL 仓库根执行：

```bash
ops build bundle
ops build win32
ops serve
```

然后在 Windows 运行：

```powershell
# 默认请求 http://127.0.0.1:8000/tela-dev/latest.json
.\tela-win32-sdk.exe --port 8000 --verbose

# 非默认端口或其他开发机地址。
.\tela-win32-sdk.exe --port 8123 --verbose
.\tela-win32-sdk.exe --bundle-index http://192.168.1.8:8123/tela-dev/latest.json --verbose

# 无窗口地校验一个本地开发包，适合构建机或排障。
.\tela-win32-sdk.exe --verify-bundle .\tela-demo.tela
```

`--port` 与 `--bundle-index` 互斥：前者只替换本机 `127.0.0.1` 端口，后者允许显式选择完整 HTTP(S) 资源索引。默认地址依赖 WSL 的 localhost 转发；局域网或后续 Mac 壳必须提供显式可达的开发服务器、访问控制和 TLS 方案，当前不把这些网络策略隐藏在 SDK 内。

`--verbose` 输出启动索引、缓存路径、网络或缓存来源、下载耗时、WASM compile/init 耗时及初始化 fuel 消耗。fuel 是每次 guest entrypoint 的执行上限：首帧和普通事件各为 `50M`，输出读取另有 `1M` 小预算；超过上限会携带剩余 fuel 与 Wasm 回溯报错，而不会无期限卡住。`ops serve` 只服务 `dist/`，不会启动浏览器。

## 8. 后续平台

| 平台 | 壳职责 | 应用协议 | 当前状态 |
| --- | --- | --- | --- |
| Win32 | HWND、WGPU、Win32 输入、缓存 | `tela-app-abi` + `.tela` | 开发态已实现 |
| macOS | AppKit/NSView、Metal WGPU surface、输入、缓存 | 同一 ABI/bundle | Apple Silicon 开发态已实现；真机图形验收见 024 |
| Android | Activity/Surface、触摸、IME、生命周期 | 同一 ABI/bundle | 仅设计目标 |
| iOS | UIKit/Metal layer、触摸、IME、生命周期 | 同一 ABI/bundle | 仅设计目标 |
| WebView | WebView bridge、DOM 输入与系统能力 | browser adapter 或兼容 bundle loader | 仅设计目标 |

这些平台不预建空 crate。出现真实窗口、输入和发布约束时，各自增加一个薄 SDK，并复用 `tela-app-abi`、`tela-bundle` 与 renderer；不要为了名义统一先创造共享大接口。

## 9. 验收边界

- `ops build bundle` 可从干净的 `dist/` 产出 release guest 包，并在 Wasmtime 首帧与 viewport 校验通过后最后发布索引。
- `ops build win32` 交叉编译 GNU Windows 壳到 `dist/win32/`。
- `ops build macos` 在 Apple Silicon macOS 构建 `dist/macos/Tela.app`；App 只含本地壳，仍启动时请求 bundle。
- 壳对 ABI、SHA-256、路径和缓存回退有自动化测试；`ShellLifecycle` 对启动/关闭、最小化、重绘合并、surface retry、device-loss 额度与文本焦点边沿有无 HWND 单测；WASM guest export 可由构建产物验证。
- Windows 真机启动时应先出现可关闭的 loading 窗口，随后显示完整 `UiFrame`，并响应鼠标、键盘、Tab/方向焦点和窗口失焦恢复；`--verbose` 不应再出现 `No DisplayHandle is available`。WSL 交叉编译本身不替代该验收。
- macOS 真机验收应使用显式 `--bundle-index` 指向开发机的可达地址，验证 AppKit/Metal 呈现、resize、输入、cache fallback 和 WGPU display handle；详细步骤见 [024](024-macOS开发SDK实施目标.md)。
