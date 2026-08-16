# 027-Android 移动端实施

> **状态：🟡 source、mobile bundle、ABI 和构建工作流已实现；Android x86_64 cross-APK/真机图形验收等待项目 Android 工具链。**
> 本文是 Android 的具体实施记录，不把它外推成 iOS、WebView、TUI 或游戏的通用宿主设计。

## 1. 结论

Android 不是对桌面文件管理器加 media query，也不是 `tela-native-sdk-runtime` 的另一个 feature。首个
移动应用由独立 Guest、独立 mock domain、独立移动 Presentation 和独立 Target Host 组成；它们只在
真正不变的 Kernel、ABI、bundle 校验和 `UiFrame` 上复用实现。

```text
tela-mobile-demo (独立移动应用)
  -> app-wasm / tela_app_abi::export_guest!
  -> dist/tela-mobile/latest.json + tela-mobile-demo.tela
  -> tela-android-sdk (strict remote loader + Wasmtime GuestRuntime)
  -> GameActivity Kotlin 壳 / Winit Android event loop
  -> Vulkan-only WGPU surface
  -> Android screen
```

这个切分落实以下判据：

| 关系 | 判定 | 当前做法 |
| --- | --- | --- |
| 默认布局、更新、焦点、交互 | 绝对一致 | 复用 `tela-core` 和 `DefaultApplicationProfile` |
| WASM 输入、状态、帧和 bundle 完整性 | 语义一致 | 复用 `tela-app-abi`、`tela-bundle`、`tela-guest-runtime` |
| Activity、surface、触摸、IME、Back | 仅概念一致 | Android 自己实现，不抽 `Host` trait 或 mobile runtime |
| 移动文件浏览 UI 与 desktop 文件管理器 | 仅概念一致 | `tela-mobile-demo` 重新组织页面和 mock domain，不引用 `tela-demo` |

## 2. 物理边界

| 位置 | 拥有内容 | 明确不拥有 |
| --- | --- | --- |
| `crates/tela-mobile-demo` | 文件夹/文本/元数据/图标静态 mock domain、移动浏览/预览/查询状态、触摸优先 Presentation | desktop 页面、桌面业务 Store、Activity 或 GPU API |
| `crates/tela-android-sdk` | `android_main`、Winit GameActivity event loop、Vulkan surface、触摸归一化、JNI text bridge、system Back | 缓存 fallback、desktop lifecycle、业务 Guest 静态链接 |
| `crates/tela-guest-runtime` | `GuestRuntime`、strict index/archive 验证、Wasmtime、无窗口 verifier | window/surface/IME/cache/Target SDK |
| `android/` | Kotlin `TelaActivity`、Manifest、Gradle、APK packaging | UI 业务布局、WASM ABI、渲染命令生成 |
| `ops` | mobile channel 构建/校验、cargo-ndk/Gradle 调度、产物发布 | Android 生命周期或 bundle 运行时校验实现 |

`ops check` 已有两条强制规则：`tela-android-sdk` 不能静态依赖 `tela-demo` 或 `tela-mobile-demo`；
`tela-guest-runtime` 不能反向依赖 Android、WebView、Win32 或 macOS SDK。未来新增游戏或 TUI Guest 时，
它们应当沿同一 delivery 边界加入，而不是被 Android host 或 CRUD 应用依赖闭包吸收。

## 3. 移动应用而非响应式 desktop

`tela-mobile-demo` 只依赖 `tela-contract`、`tela-core`、字体/图标/文字能力和可选 ABI macro；Cargo 图上不
依赖 `tela-demo`、`tela-ui` 或 `tela-widgets`。它目前提供：

- 顶部标题、返回、搜索、文件夹/文档/资源图标与 48dp 以上操作目标；
- 文件夹浏览、返回层级、静态文本/元数据/图标预览；
- 查询过滤和 Escape/Back 的应用级规则；
- 与 desktop demo 无关的内置 mock domain。

这不是“把桌面树、表格、工具栏缩窄”。移动页面可以随着真实场景重新决定导航、信息密度和状态，
只要仍通过 `AppEvent` / `AppStatus` / `UiFrame` 与 Target 通信。图片资源、复制粘贴、上传、真实文件
系统和跨页面路由不在首期能力范围。

## 4. Bundle 与启动

`ops build bundle mobile` 总是编译 release WASM，并在发布 index 前以 `tela-guest-verify` 完成 archive、
ABI、首帧和 viewport 校验：

```text
dist/tela-mobile/
├── latest.json
└── tela-mobile-demo.tela
```

Android 启动时只接受 Gradle 注入的完整 HTTP(S) `telaBundleIndex`。加载顺序为：

1. `TelaActivity.onCreate` 在 `GameActivity` 初始化前配置 URL；空 URL 保留原生诊断页。
2. Rust `android_main` 创建 Winit event loop 和后台加载 worker。
3. worker 读取 index，检查格式、ABI、bytes、SHA-256 和相对 archive URL；下载 archive 后再次校验内部清单。
4. worker 用 `GuestRuntime` 实例化 WASM，读取首帧/status；成功后把 runtime 交给 Android event loop。
5. host 创建或复用 Vulkan surface，派发逻辑 viewport 并呈现最新 `UiFrame`。

没有 CacheStorage、文件 cache、最后有效包、自动降级或旧 bundle fallback。网络、哈希、ABI、archive、
Wasmtime 或 frame 任一失败时都停在原生诊断页。桌面 cache 是桌面开发体验的一部分，不能因为 Android
也使用 Wasmtime 就漏进移动端。

## 5. Activity、渲染和生命周期

`TelaActivity` 继承 `androidx.games.activity.GameActivity`，Rust 端通过 Winit re-export 的 Android
GameActivity API 接收 `AndroidApp`。不使用 `NativeActivity`，也不在 Rust 里自己复制 GameActivity glue。
Native library 的名称是 `main`，由 Kotlin `System.loadLibrary("main")` 加载。

渲染只请求 `wgpu::Backends::VULKAN`：

- `minSdk = 29`，`compileSdk = 36`，`targetSdk = 36`；
- Manifest 要求 Vulkan feature；首期只构建 `x86_64`；
- 没有 GLES 回退，也不把 Vulkan 失败伪装成软件 renderer 成功；
- `resumed` 创建 `Window`、surface、adapter、device、renderer；
- `suspended` 在回调返回前 drop GPU surface 和 window，保留 portable Guest；
- 重新 `resumed` 后重新创建 surface 并向同一 Guest 发送当前 viewport。

`WgpuRenderer` 继续只消费 `UiFrame`。Android host 的 loading/error frame 也通过相同 renderer 绘制，
但它不进入 Guest 的业务代码。

## 6. 输入、IME 与 Back

### 6.1 触摸

只接受第一个 active pointer：

1. `Started` 不立即向 Guest 发送 down。
2. 移动距离未越过 density-adjusted touch slop 时，release 才顺序发送 `PointerDown`、`PointerUp`。
3. 越过 slop 后不发送 click；之后只发送方向反转后的内容 `PointerScroll` delta。
4. `Cancelled` 或第二手指不产生延迟 click，也不抢占 active gesture。

首期没有惯性、pinch、缩放、长按、拖放或多指手势。这是刻意收窄的输入协议，不是假装完整的鼠标模拟。

### 6.2 原生文本通道

Kotlin 创建 1x1 隐藏受控 `EditText`，Rust 维护下面的极小桥接状态：

| 能力 | 首期行为 |
| --- | --- |
| 文本值 | Kotlin 与 Guest 交换完整 UTF-8 值；不传 delta |
| 焦点 | `AppStatus.input_focused` 控制 `EditText` focus 与 IME 显示 |
| 中文 IME | 每次 native 文本段以 `InputCompositionStart`/`InputCompositionEnd` 包裹 |
| 光标 | host 同步值后放在末尾 |
| 完成键 | `IME_ACTION_DONE` 转为 Guest `InputEnter` |
| 未支持 | selection、copy/paste、候选框位置、多段 composing range、富文本 |

`SetInputValue` 仍是唯一可见文本写入语义。composition 标记只表明系统 IME 生命周期，不能被组件误解为
已提交的业务值。

### 6.3 系统 Back

Back 由 Target 和 Guest 分工，顺序不能交换：

1. input active 时 host 先派发 `InputBlur`，Kotlin 隐藏 IME，不向 Guest 派发 Escape。
2. 非文本状态时 host 派发 physical Escape。
3. mobile Guest 按预览返回、文件夹返回、清空查询、根目录未处理的顺序消费 Escape。
4. 只有根目录未处理才设置 finish 标记，Kotlin 轮询该标记后调用 `finish()`。

这使系统 Back 保留 Android 的退出责任，同时把业务导航留在 Guest，而不是让 Activity 知道文件夹或预览状态。

## 7. 构建、服务与验收

项目 Android 工具链需要由项目环境提供：`cargo-ndk`、Rust `x86_64-linux-android` target、Android NDK、
Android SDK API 36、JDK 17 和 Gradle。不要用用户级 rustup 临时安装 target 绕过项目环境隔离。

```bash
ops check
ops build bundle mobile
ops verify bundle mobile
ops serve 8001
ops build android --bundle-index http://<development-host>:8001/tela-mobile/latest.json
```

连接真机或另一台 emulator 时，将 `<development-host>` 替换为设备可达的局域网地址，不能使用开发机的
`127.0.0.1`。`ops build android` 先构建/校验 mobile bundle，再执行：

```text
cargo ndk -t x86_64 -o android/app/src/main/jniLibs build --release -p tela-android-sdk
gradle --no-daemon :app:assembleDebug -PtelaBundleIndex=<URL>
```

最终 APK 为 `dist/android/tela-mobile-debug.apk`。构建任一步失败即停止，不发布 APK。

自动化证据：`cargo test -p tela-android-sdk` 覆盖 whole-value IME 和触摸 adapter；`ops` tests 覆盖 mobile
channel、strict verifier、invalid URL、native failure 不启动 Gradle 和不发布 APK。实际 x86_64 cross-target
编译与设备验收仍需在有 Android 工具链的环境执行，验收包括 Vulkan surface、resume/suspend、tap/scroll、
中文 IME、Back、远程 bundle 正常/失败两条路径。

## 8. 下一次抽取判据

现在不创建 Android/iOS 共用 runtime、通用 mobile component crate 或动态插件 ABI。只有出现第二份真实实现
后才判断：

- Android 与 iOS 的 bundle 加载是否绝对一致，足以复用代码；
- 两端 IME 或 Back 是否仅语义一致，值得定义窄协议；
- TUI、游戏或另一个移动应用是否只是同一概念，因而只能共享 World Map 边界；
- 新能力能否由 app + provider + target 加入，而不修改既有 Guest、Renderer 或 Kernel crate。

最后一条是 TUI/游戏思想实验的可执行版本：未来接入新 Target 时，修改已有共享 crate 并不是默认路径；
若不得不修改，必须指出被证明错误的共享假设，而不是以平台 `cfg` 分支掩盖它。

关联文档：[026-架构迭代方案](026-架构迭代方案.md)、[023-平台SDK与WASM开发包](023-平台SDK与WASM开发包.md)、[022-构建产物与浏览器宿主目录](022-构建产物与浏览器宿主目录.md)。
