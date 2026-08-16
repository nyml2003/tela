# 024-macOS 开发 SDK 实施目标

> **状态：✅ 已在 Apple Silicon macOS 真机（M3 Pro，macOS 26.6）完成首次编译与图形验收：`ops build macos` 产出 `Tela.app`，`ops build bundle` + `ops serve 8001` 提供开发包，窗口正常呈现；`ops check` 四道门全过（2026-08）。**
>
> 本文只定义开发态 AppKit 壳。签名、公证、DMG、沙盒分发与完整原生文本服务统一归档在 [011](011-已知限制与未来扩展清单.md) 第 5 节。

## 1. 结论

macOS SDK 与 Win32 SDK 共享 **应用包协议、中性 Wasmtime guest runtime 和桌面生命周期策略**，但不共享一个伪统一的窗口 `Host` trait。macOS crate 只拥有 AppKit、Metal/WGPU surface、输入归一化和本机缓存：

```text
WSL / 任意开发机                         Apple Silicon Mac
────────────────────                     ───────────────────────────────
ops build bundle                          Tela.app 启动
  -> dist/tela-dev/latest.json                -> 一次 HTTP GET latest.json/archive
  -> dist/tela-dev/tela-demo.tela             -> SHA-256 / ABI / archive 校验
ops serve 8001 (0.0.0.0)                      -> ~/Library/Caches/tela/.../last-valid.tela
                                                     -> Wasmtime guest（本机）
                                                     -> UiFrame
                                                     -> WGPU / Metal / NSView
```

WASM **不**被复制进 `Tela.app`；每个 App 进程只在启动阶段请求一次开发包。网络或校验失败时，才回退同一台 Mac 上最后一个通过全部校验的 archive。没有轮询、热更新、后台预取、跨版本状态迁移或 Mac 与 WSL 间的 RPC。

这保留了很重要的边界：Mac 只需要本地构建原生壳，应用源码与 bundle 仍可以由 WSL 构建和提供；以后接系统桥时，Swift/Objective-C 能力也自然留在这个壳中，不污染 `tela-core` 或 WASM ABI。

## 2. crate 与线程所有权

| 位置 | 拥有 | 不拥有 |
| --- | --- | --- |
| `tela-guest-runtime` | 严格 `.tela` archive 校验、Wasmtime `GuestRuntime`、无窗口 verifier | AppKit、HWND、WGPU surface、平台输入、cache、lifecycle |
| `tela-native-sdk-runtime` | 桌面 `.tela` 获取/缓存策略、启动 CLI、`ShellLifecycle` | Wasmtime 核心执行、AppKit、HWND、WGPU surface、平台输入 |
| `tela-macos-sdk/src/appkit.rs` | `NSApplication`、`NSWindow`、定时轮询、关闭顺序 | bundle 解包、应用业务状态 |
| `tela-macos-sdk/src/view.rs` | `NSView`、主线程 guest dispatch、焦点边沿、指针/键盘、重绘与 surface retry | 后台 HTTP、全局业务 Store |
| `tela-macos-sdk/src/gpu.rs` | AppKit raw handles、WGPU/Metal device/surface、device-loss 回收 | UI DSL、布局、业务资源语义 |
| `tela-macos-sdk/src/startup.rs` | 一次后台下载和 guest 初始化、`~/Library/Caches/tela/development/last-valid.tela` | 任意 AppKit/WGPU 对象 |
| `tela-win32-sdk` | 同一协议之上的 HWND/Windows 输入实现 | macOS 专有对象 |

`NSWindow`、`NSView`、WGPU surface/device/renderer、`UiFrame` 和所有 guest dispatch 都只在 AppKit 主线程。后台 worker 只返回 `Result<GuestRuntime, String>`，窗口关闭时先置取消位；迟到结果不会触碰已经释放的 native view。

## 3. 生命周期和恢复

共享 `ShellLifecycle` 维护 `Loading -> Running/Suspended -> Failed/Closing`。平台壳负责把这些状态落实为本机资源操作：

| 场景 | macOS 壳行为 |
| --- | --- |
| 启动 | `NSWindow` 立即展示 loading 文案；网络和 Wasmtime 编译在命名 worker 中完成，主 run loop 保持可关闭。 |
| 有效 guest + 非零 view | 主线程创建 AppKit raw-handle WGPU surface，派发 viewport，开始绘制。 |
| 最小化或零尺寸 | 不向 guest 发送零 viewport，也不 acquire/configure 零尺寸 drawable；恢复尺寸后重新派发 viewport。 |
| window focus / guest input focus | 只有两个条件均真才补发一次 `InputFocus`；离开、失焦、失败或关闭最多补发一次 `InputBlur`。 |
| AppKit view resize | 使用逻辑 points 派发 viewport；使用 `convertSizeToBacking` 的像素尺寸配置 WGPU surface。 |
| `Timeout` | 使用共享的 `16/32/64/128/250ms` 单一 retry，成功 present 后复位，避免 busy loop。 |
| `Outdated` / `Suboptimal` | 读取最新非零 size 并重新 configure。 |
| `Lost` | 在同一 WGPU instance 上重建 surface，再派发 viewport。 |
| device lost | callback 只记录 generation 和诊断；主线程最多整体重建一次 GPU，第二次或重建失败显示原生错误页。 |
| 关闭 | 取消 startup worker、清理 hover/text focus/retry，再释放 GPU；不接受迟到 worker 结果。 |

WGPU 30 创建可 present surface 时要求 instance 与 surface 给出匹配的 display owner。macOS 实现显式使用 `AppKitDisplayHandle` 和 `AppKitWindowHandle(NSView)`，不依赖隐式 display lookup；这与 Win32 的 explicit display-handle 修复采用同一原则。

## 4. 输入范围

v1 将 AppKit 事件规范化为现有 `tela-app-abi::AppEvent`：

- 鼠标移动、按下、抬起、离开和滚轮；坐标为以左上角为原点的逻辑 points；
- USB-HID physical key、Shift/Control/Option/Command modifier；应用层继续解释键盘意图、组合键和可运行时替换的 keymap；
- Tab、方向键、Enter、Escape、Backspace 和可打印 ASCII 的受控输入；
- `AppStatus.cursor` 映射为 arrow / I-beam / pointing-hand cursor。

中文/日文 IME、死键、剪贴板、拖放、原生 accessibility tree 和系统桥尚未实现。特别是当前没有把 `keyDown` 伪装成完整文本服务：ASCII 输入明确是开发态最小子集。后续应由 `tela-widgets` 的文本原子定义 composition/selection/commit 契约，再在 AppKit `NSTextInputClient` 通道实现；完整后置清单见 [011](011-已知限制与未来扩展清单.md) 第 5 节。

## 5. 构建产物

`ops build macos` 仅在 Apple Silicon macOS 运行，并生成：

```text
dist/macos/Tela.app/
└── Contents/
    ├── Info.plist                         由 crates/tela-macos-sdk/resources/Info.plist 复制
    └── MacOS/
        └── tela-macos-sdk                 aarch64 原生 AppKit/WGPU 壳，权限 0755
```

应用包、WASM 和资源不在此目录；`dist/` 已被 Git 忽略，删除后可分别由 `ops build bundle` 与 `ops build macos` 恢复。v1 不提供 `.icns`、codesign、notarization 或 DMG，因此 Finder 首次打开未签名开发 App 时应使用开发者允许路径；生产分发另立目标。

## 6. Mac 开发命令

### 6.1 提供 bundle 的机器（例如 WSL）

```bash
ops build bundle
ops serve 8001
```

`ops serve` 监听 `0.0.0.0`。从 Mac 访问时使用 **Mac 能到达的开发机地址**，例如 Windows/WSL 宿主的局域网 IP；不要把 `127.0.0.1` 原样带到另一台机器。防火墙、公司网络和 WSL 网络模式属于开发环境责任，SDK 不猜测地址或绕过网络策略。

### 6.2 Apple Silicon Mac

首次准备本机编译环境：

```bash
xcode-select --install
git clone <你的 tela 仓库地址>
cd tela
nix develop
pnpm --dir ops install
ops build macos
```

然后以显式远程 bundle 启动：

```bash
open dist/macos/Tela.app --args \
  --bundle-index http://<开发机可达IP>:8001/tela-dev/latest.json \
  --verbose
```

`--port 8001` 只生成 `http://127.0.0.1:8001/...`，适合同一台 Mac 上运行 `ops serve`；跨机器必须使用完整 `--bundle-index`。`--verbose` 会打印索引、缓存路径、network/cache 来源、下载耗时和 guest compile/init 指标。无窗口校验 archive 时也可以执行：

```bash
dist/macos/Tela.app/Contents/MacOS/tela-macos-sdk --verify-bundle /path/to/tela-demo.tela
```

## 7. 验收与边界

Mac 真机首次验收至少应覆盖：

1. `nix develop && ops build macos` 产出可由 `open` 启动的 `Tela.app`；`plutil -lint` 通过。
2. Mac 通过 `--bundle-index` 从 WSL/开发机获取网络包，随后断网重启可使用本地最后有效 cache。
3. loading 页在慢网时保持窗口可移动、可关闭；错误时显示原生诊断而不崩溃。
4. AppKit/Metal 成功呈现完整 `UiFrame`，resize、最小化/恢复、hover、鼠标、滚轮、Tab/方向键与 ASCII input 正常；没有 WGPU display-handle 错误。
5. 人为触发/记录 device loss 时，最多完成一次 GPU 重建；第二次走有序 native error page。

在 Linux/WSL 中可验证共享 runtime、bundle verifier、生命周期和 ops 单测，但不能证明 AppKit 或 Metal 链接、窗口事件和实际 drawable。不要把交叉构建成功当作 macOS 图形验收。

## 8. 后续

- Intel macOS 与 universal binary：单独定义 target 与 bundle 合并策略，当前不在 aarch64 v1 范围。
- IME/clipboard/accessibility：先扩充 `tela-widgets` text channel 契约，再实现原生适配。
- 系统桥：在 `tela-macos-sdk` 追加窄能力模块或稳定 C/Swift bridge；不替代 `.tela` 应用协议。
- 正式分发：icon、签名、公证、DMG、自动更新、TLS/认证和开发包信任策略另立发布设计。
