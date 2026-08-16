# 028-iOS 开发 SDK 实施

> **状态：实现代码、Xcode 工程、`ops` 构建/部署流程和本地单元测试已落地；Apple Silicon macOS 上的 iPhoneOS 链接、Apple Development 签名、真机安装与图形验收尚待执行。** 本文只定义 iPhone 开发态，不承诺模拟器、iPad、App Store 分发或远程应用更新。

## 1. 结论与边界

iPhone 首期是一个本地静态代码闭包：`tela-ios-sdk` 直接链接 `tela-mobile-demo` 的
`native-app` API，使用 Winit/UIKit 生命周期和 Metal/WGPU 绘制。它**不**链接
`tela-guest-runtime`、`tela-native-sdk-runtime`、`tela-app-abi` 或 `tela-bundle`，也不下载
`.tela`、不启动 Wasmtime、不保留动态包缓存。

这不是把 macOS 桌面文件管理器移植到 iPhone。`tela-mobile-demo` 已经拥有独立的移动 mock
domain 和移动 presentation；iOS 链接的是这一份移动业务视图，`tela-demo` 与桌面 runtime 均不进入
iPhone 依赖闭包。

```text
tela-mobile-demo (native-app, mobile domain + mobile presentation)
  -> tela-ios-sdk (Winit/UIKit lifecycle, touch, keyboard, safe area, Metal/WGPU)
  -> libtela_ios_sdk.a (aarch64-apple-ios)
  -> ios/TelaMobile.xcodeproj (thin Objective-C main + safe-area helper)
  -> signed iPhone app
```

首期硬约束如下：

- `aarch64-apple-ios`、iOS 16.0+、`TARGETED_DEVICE_FAMILY=1`，仅 iPhone。
- `ValidOrientations::Portrait` 与 `Info.plist` 均锁定竖屏。
- UIKit `safeAreaInsets` 以逻辑 point 传给移动应用；根 Frame 预留顶部状态栏、底部 Home indicator 和横向避让区。
- Winit 的 physical touch 坐标先按 scale factor 转为 point。一个活动指针在 12pt slop 内延迟为 tap，越过 slop 后只派发滚动，不生成点击。
- Winit/UIKit 的 `UIKeyInput` 驱动软件键盘。宿主维护完整受控字符串，文本插入和退格再回写 `MobileApp`；Return 收起键盘并保留查询。

移动页面沿用已实现的 64pt app bar、52pt 搜索输入和 72pt 列表行，所有可点击区域保持至少 44pt 的触控尺度；安全区不会被当作可点击或可绘制的业务内容区域。

## 2. 所有权

| 位置 | 拥有内容 | 不拥有内容 |
| --- | --- | --- |
| `crates/tela-mobile-demo` | 移动文件浏览应用、独立领域、移动视图、portable `UiFrame`、直接 `MobileApp` 会话 | UIKit、Metal、Winit、桌面业务视图 |
| `crates/tela-ios-sdk/src/ios.rs` | UIKit lifecycle、窗口、Metal surface、键盘、触控、重绘和 surface 恢复 | 下载、bundle、业务 Store、macOS AppKit |
| `crates/tela-ios-sdk/src/safe_area.rs` | 从 Winit `UiKitWindowHandle` 调用窄 C helper 读取逻辑安全区 | 通用跨平台 safe-area trait |
| `ios/TelaMobile` | Objective-C `main`、UIKit safe-area helper、Info.plist | Rust 业务逻辑、Rust 工具链、签名身份 |
| `ios/TelaMobile.xcodeproj` | iPhone target、系统 framework 链接、Apple Development 签名配置 | bundle delivery、Android/desktop 工程 |
| `ops` | 静态库 staging、无签名构建、签名后 `devicectl` 安装/启动顺序 | 自动选择 Team、设备授权、生产证书 |

没有抽取 Android/iOS 公共 `Host`、`MobileRuntime` 或输入适配器。两端的触控算法恰好相似，但代码和生命周期均在各自 Target crate 中；只有 `UiFrame`、`PointerEvent`、`Insets` 与移动应用本身是有意复用的稳定契约。

## 3. 构建环境

必须在 Apple Silicon macOS 运行完整 Xcode。默认 Darwin dev shell 使用 Nix 的 macOS SDK，不能用于
iPhoneOS；专用 `.#ios` shell 提供下列项目级命令，自动切换到完整 Xcode：

```bash
nix develop .#ios --command tela-ios-bootstrap
nix develop .#ios
ops build ios
```

`tela-ios-bootstrap` 在 `${XDG_CACHE_HOME:-$HOME/.cache}/tela/ios` 安装项目私有 Rust toolchain，并只添加
`aarch64-apple-ios` target。`tela-ios-cargo` 为该 target 设置完整 Xcode 的 iPhoneOS SDK、clang、ar 和
deployment target 16.0；不写用户级 `rustup`。完整 Xcode 不在默认位置时，设置
`TELA_IOS_DEVELOPER_DIR=/path/to/Xcode.app/Contents/Developer` 后再运行上述命令。

`ops build ios` 的顺序固定：

1. `tela-ios-cargo build --target aarch64-apple-ios -p tela-ios-sdk` 生成 `libtela_ios_sdk.a`。
2. 静态库复制到 `ios/build/rust/libtela_ios_sdk.a`，这是 checked-in Xcode 工程引用的唯一生成路径。
3. `tela-ios-xcodebuild` 对 `TelaMobile` 运行 `-sdk iphoneos CODE_SIGNING_ALLOWED=NO build`。
4. 无签名设备 App 位于 `ios/build/DerivedData/Build/Products/Debug-iphoneos/TelaMobile.app`。

`ios/build/` 是 Xcode 的项目私有 DerivedData/staging 区，已被 Git 忽略；它不同于浏览器和 Android 的
`dist/` 发布目录，因为签名与设备安装直接消费 Xcode 的本地 App bundle。

## 4. 签名与真机部署

首次部署前，在 macOS 用 Xcode 打开 `ios/TelaMobile.xcodeproj`，为 `TelaMobile` target 选择一个可用的
Apple Development Team。工程不会提交任何 Team ID、provisioning profile 或私钥。

连接已在 Xcode 信任的 iPhone 后，以明确 UDID 运行：

```bash
nix develop .#ios --command ops build ios
nix develop .#ios --command ops ios deploy --device <UDID>
```

部署命令重新以 Xcode 当前 Team 构建，允许 Xcode 更新 provisioning，然后依次执行：

```text
xcrun devicectl device install app --device <UDID> <TelaMobile.app>
xcrun devicectl device process launch --device <UDID> dev.tela.mobile
```

`--device` 是必填项，避免多设备环境中安装到错误手机。签名失败时应先在 Xcode 处理 Team、设备信任和
Development certificate；CLI 不会尝试创建或修改这些账户级状态。

## 5. 真机验收

下列项目是 macOS 收尾验收项，当前没有将它们声明为已通过：

1. `nix develop .#ios --command ops build ios` 成功生成无签名 ARM64 iPhone App，`plutil -lint ios/TelaMobile/Info.plist` 通过。
2. 已配置 Team 后，`ops ios deploy --device <UDID>` 能安装并启动 `dev.tela.mobile`。
3. 带刘海和 Home indicator 的设备中，app bar、搜索框和最后一项列表均不被系统区域遮挡；旋转不会进入横屏布局。
4. 点击搜索框会显示系统键盘；中文/英文输入、Unicode 退格和 Return 后的 blur 保持受控查询一致。
5. tap 打开预览，纵向拖动只滚动列表，不误触点击；多指与 cancel 不留下悬挂 tap。
6. 进入后台再返回会释放并重建 Metal surface，移动应用状态可继续渲染；丢失/过期 drawable 不导致进程崩溃。

## 6. 非目标与后续

- 不支持 simulator、iPad、横屏、universal binary、Android/iOS 共用 native host，或 iOS 上的动态 WASM bundle。
- 不提供 App icon、App Store archive/export、TestFlight、production signing、自动更新、网络热更新、缓存回退或远程资源下载。
- 文本通道首期只覆盖完整值输入和退格；选择范围、复制粘贴、候选框锚点、拖放和 accessibility tree 仍需先扩充 `tela-widgets` 契约。
- 真机 Metal/WGPU 指标、耗电/内存预算与崩溃采集将在完成首次物理验收后再以真实证据确定优化方向。
