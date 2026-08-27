# tela — 通用 UI 基座

> tela，拉丁语，意为"画布/织布"。一套与业务无关的底层 UI 基础设施，同一套基座同时支撑游戏 HUD、游戏内编辑器、B 端 CRUD 后台、C 端交互页面。

## 定位

- 可嵌入已有程序内部运行，也可独立运行；支持多运行环境（游戏引擎内嵌、浏览器画布、离线渲染）。
- UI 只负责界面渲染与用户交互采集，**不执行业务逻辑、IO、网络、存储**，所有副作用全部交给外部宿主处理。
- 基座只提供绘图原语与基础设施，**不内置任何业务组件**；按钮/输入框/表格等由上层组件套件实现。

## 阅读路径

按顺序阅读，每个文档可独立阅读：

| 文档 | 回答的问题 | 什么时候看 |
|---|---|---|
| [001-痛点与目标](docs/001-痛点与目标.md) | 为什么建这个基座？要解决什么？边界在哪？ | 第一次接触，或忘了"为什么" |
| [002-架构总览与分层](docs/002-架构总览与分层.md) | 现在的分层与 crate 归属长什么样？依赖图、数据流、交付链与腐化审计 | 想理解系统归属、做结构性决策时（**现状权威**） |
| [003-场景树与节点模型](docs/003-场景树与节点模型.md) | UI 树长什么样？节点 id 怎么分配？构建期校验什么？ | 写树相关代码前 |
| [004-更新策略与状态保持](docs/004-更新策略与状态保持.md) | 整体重建和局部 Dirty 怎么共存？视图状态怎么保住？ | 做增量更新、处理焦点/滚动保持时 |
| [005-key身份策略](docs/005-key身份策略.md) | 4 种 key 策略分别怎么用？稳定身份怎么分配？ | 做动态列表、拖拽面板、数据驱动表格时 |
| [006-布局引擎](docs/006-布局引擎.md) | Row/Column/Wrap/Stack 怎么组合？单次测量如何保证？约束模型、叠层和虚拟列表怎么做？ | 实现或接入布局能力时 |
| [007-绘制与渲染后端](docs/007-绘制与渲染后端.md) | 绘制原语有哪些？命令怎么跟后端解耦？不支持的特效怎么降级？ | 接新渲染后端或加视觉原语时 |
| [008-交互焦点与宿主接口](docs/008-交互焦点与宿主接口.md) | 输入怎么进树？双焦点模型怎么共存？模态怎么拦截？宿主接口长什么样？ | 做输入、焦点、弹窗时 |
| [009-多环境集成](docs/009-多环境集成.md) | 嵌入/浏览器/离线三种宿主怎么适配？为什么基座不接管主循环？ | 接新宿主时 |
| [010-落地路线](docs/010-落地路线.md) | 现在先做什么？每阶段怎么验收？ | 不知道从哪下手时 |
| [011-已知限制与未来扩展清单](docs/011-已知限制与未来扩展清单.md) | v1 明确不做什么？后置了什么？ | 排需求、定边界时 |
| [012-业务数据绑定](docs/012-业务数据绑定.md) | 表单输入怎么跟宿主数据双向同步？BindId 是什么？ | 做表单、动态列表、绑定组件时 |
| [013-决策日志](docs/013-决策日志.md) | 每个设计决策为什么这么定？被什么取代了？ | 回溯决策、评审设计时 |
| [014-文件管理器演示与组件运行时](docs/014-文件管理器演示与组件运行时.md) | 全视口客户端演示如何以 MVC、组件契约和局部 Dirty 刷新落地？ | 实现或评审文件管理器演示时 |
| [015-tela-ui 与局部交互状态](docs/015-tela-ui与局部交互状态.md) | 分子组件层、Signal 精准更新、局部草稿和调度如何分层？ | 设计或实现 `tela-ui` 时 |
| [016-已解决问题归档](docs/016-已解决问题归档.md) | 哪些历史问题已经有源码和测试证据，不应继续作为待办？ | 清理路线图和限制清单时 |
| [017-tela-ui 第一阶段实施目标](docs/017-tela-ui第一阶段实施目标.md) | 下一阶段如何迁移分子组件、建立 UiIntent 与 Toolbar 样板？ | 启动 `tela-ui` 第一阶段 Goal 时 |
| [018-tela-ui 第二阶段 DraftInput 与局部状态](docs/018-tela-ui第二阶段DraftInput与局部状态.md) | 草稿、IME、提交语义和局部状态生命周期如何落地？ | 启动 `tela-ui` 第二阶段 Goal 时 |
| [019-文件管理器视觉与图标系统实施目标](docs/019-文件管理器视觉与图标系统实施目标.md) | 文件管理器如何升级为双层原生客户端，并建立 iconfont 与图标组件套件？ | 启动视觉与图标系统 Goal 时 |
| [020-字体基线焦点与键盘意图实施目标](docs/020-字体基线焦点与键盘意图实施目标.md) | 字体和图标如何按真实基线排版？焦点、键盘意图与运行时键位表如何闭环？ | 启动输入与排版基础设施 Goal 时 |
| [021-图标原语与行内内容架构](docs/021-图标原语与行内内容架构.md) | 图标 provider、原子交互与 prefix/suffix 行内内容如何分层？ | 设计图标或文本组合时 |
| [022-构建产物与浏览器宿主目录](docs/022-构建产物与浏览器宿主目录.md) | 浏览器源码、wasm 和静态发布物如何分离？ | 改构建、服务或浏览器诊断页时 |
| [023-平台SDK与WASM开发包](docs/023-平台SDK与WASM开发包.md) | 原生壳如何一次性加载可验证 WASM 包？Win32 开发态怎样运行？ | 接原生平台 SDK、包协议或系统桥时 |
| [024-macOS开发SDK实施目标](docs/024-macOS开发SDK实施目标.md) | macOS AppKit/Metal 壳怎样在 Mac 本地构建、从 WSL 请求一次 bundle 并缓存？ | 在 Apple Silicon Mac 接入或验收开发 SDK 时 |
| [025-WebView开发SDK实施目标](docs/025-WebView开发SDK实施目标.md) | 浏览器如何作为 WebView 壳，从同一 `.tela` 包启动 WGPU 应用？ | 接浏览器、嵌入式 WebView 或排查开发态启动时 |
| [027-Android移动端实施](docs/027-Android移动端实施.md) | Android 为什么是独立移动 Guest + GameActivity Target？strict bundle、Vulkan、IME 和 Back 如何落地？ | 构建、验收或扩展 Android 移动端时 |
| [028-iOS开发SDK实施](docs/028-iOS开发SDK实施.md) | iPhone 为什么静态链接独立移动应用？UIKit/Metal、安全区、签名与真机验收如何收口？ | 在 Apple Silicon Mac 构建或验收 iPhone 开发态时 |
| [030-通用组件体系重构方案](docs/030-通用组件体系重构方案.md) | 如何以 Headless、Signal、双形态 Kit 和 Kernel 增量建立完整的通用组件目录？ | 启动或评审组件体系重构前 |
| [031-应用组合DSL与显式依赖方案](docs/031-应用组合DSL与显式依赖方案.md) | `ui!` DSL、帧期订阅与显式依赖如何确立？（历史设计记录） | 了解 DSL 演进史；开发本身以 036 为准 |
| [032-宿主桥MVP实施目标](docs/032-宿主桥MVP实施目标.md) | 桥在运行时怎么活？crate 放哪、按什么顺序落地、怎么算完成？ | 实施或扩展宿主桥时 |
| [033-DSL组件化与派生宏方案](docs/033-DSL组件化与派生宏方案.md) | DSL 如何只剩"组件"一个概念？自定义组件、derive 与指令重构怎么做？ | 写 `ui!` 组件或扩展宏时 |
| [034-组件私有状态与声明式生命周期框架改动意图](docs/034-组件私有状态与声明式生命周期框架改动意图.md) | 组件私有状态归谁？稳定身份与候选帧事务如何跨帧持续？ | 设计有状态组件时 |
| [035-变速齿轮APP产品逻辑意图](docs/035-变速齿轮APP产品逻辑意图.md) | 变速齿轮的产品形态、主链路与验收边界是什么？ | 开发 speed-gear 应用族时 |
| [036-事件系统与组件生命周期机制梳理](docs/036-事件系统与组件生命周期机制梳理.md) | 事件六层怎么分？已呈现帧闭环与组件生命周期的当前基准是什么？ | **当前开发基准**；写任何交互/组件代码前 |
| [037-视觉保真与微交互动画实施目标](docs/037-视觉保真与微交互动画实施目标.md) | 视觉差距的根因是什么？wgpu 原语补齐、Tick 动画与五端验收如何分期？ | 启动视觉/动画 Goal 时 |
| [038-CC远程会话产品意图](docs/038-CC远程会话产品意图.md) | 手机如何经中继操控桌面 Claude Code？三端拓扑、CLI 线格式实测与部署边界是什么？ | 开发 CC Remote 产品族（cc-protocol/relay/agent/app）时 |
| [039-桥net能力与移动回投实施目标](docs/039-桥net能力与移动回投实施目标.md) | guest 联网为什么走具名 `net.http.request`？跨帧回投生命周期与 fuel 预算如何闭环？ | 给宿主加网络桥、写联网 guest 或排查回投不上屏时 |

### 专题文档（未编号目录）

| 入口 | 内容 | 什么时候看 |
|---|---|---|
| [UI框架/README](docs/UI框架/README.md) | `ui!` DSL 使用参考；全部组件与 Props 见 [组件集合](docs/UI框架/组件集合/README.md) | 写 DSL 界面时查组件用法 |
| [桥/000-宿主桥总览](docs/桥/000-宿主桥总览.md) | 宿主桥统一规则：分组模型、原子性、版本与传输；各桥语义在其子目录（[通用模型](docs/桥/通用模型/README.md)、base/device/position/config） | 设计或实现新桥时 |
| [win32开发经验/](docs/win32开发经验/) | Win32 平台踩坑实录（光标冻结、WM_SETCURSOR 等） | 排查 Win32 宿主问题时 |

## 开发工作流（ops）

日常开发统一走 [ops](ops/README.md)（DDD 分层 CLI，运行时零依赖，Node 24 直接跑 TS）。
进入本仓库的 nix dev shell（direnv/flake）后，flake 提供的项目级 `ops` 命令即可用；
不要安装用户级同名入口。未在 nix 环境时，可在仓库根执行 `node ops/src/interface/cli.ts`。

```bash
ops check                # 四道验证门（fmt/clippy/test/依赖方向）
ops build core           # 只检查 contract、core 与 ui-foundation 的纯 Rust 产品闭包
ops build webview        # 构建 desktop guest、WebView Target host 与浏览器静态资产
ops build win32          # 在 WSL 交叉构建 Win32 开发壳到 dist/win32/
ops build macos          # 在 Apple Silicon macOS 本机构建 Tela.app 到 dist/macos/
ops build bundle mobile  # 构建独立的 tela-mobile guest bundle
ops verify bundle mobile # 校验独立的 tela-mobile bundle
nix develop .#android --command tela-android-bootstrap
                         # 首次准备项目私有 Android 工具链缓存
nix develop .#android    # 显式进入 Android 工具链；默认 shell 不安装 SDK/NDK
ops build android        # 在 WSL 构建 arm64-v8a Vulkan GameActivity APK
ops android serve        # 固定监听 WSL 127.0.0.1:8000，供 USB adb reverse 使用
ops android deploy --serial <serial>
                         # 调 Windows Android Studio 的 adb.exe 安装并启动真机 APK
nix develop .#ios --command tela-ios-bootstrap
                         # 首次准备项目私有 Apple Silicon iPhone Rust target
nix develop .#ios --command ops build ios
                         # 在 Apple Silicon macOS 构建无签名 ARM64 UIKit/Metal App
nix develop .#ios --command ops ios deploy --device <UDID>
                         # 使用 Xcode 已配置 Team 安装并启动真机 App
ops verify [bundle|gpu]  # 默认校验 desktop 应用包；bundle 可指定 desktop|mobile
ops serve                # 开发静态服务器（http://127.0.0.1:8000/）
ops build relay            # 构建 CC Remote 中继到 dist/cc-relay/（std 线程零 tokio，常驻 ~4MB）
ops build agent           # 构建桌面 agent 到 dist/cc-agent/（子进程驱动 claude CLI）
TELA_CC_RELAY_URL=http://<relay>:8787 TELA_CC_TOKEN=<token> ops build cc
                          # 构建 CC Remote 手机端（cc bundle + APK，注入中继配置）
```

`dist/` 是浏览器、动态包与既有平台工件的发布目录，已被 Git 忽略；浏览器静态文件、`tela-dev` WASM 包、
`tela-mobile` WASM 包、Win32 壳、macOS `Tela.app` 和 Android APK 都必须由受控 `ops` 命令生成。iPhone
App 则由 Xcode 在已忽略的 `products/ios/build/` 生成，以便直接签名和安装。浏览器页面模板与宿主代码位于
`products/webview/`，desktop/mobile Rust 业务应用分别位于 `apps/desktop-demo/`、`apps/mobile-demo/`；动态
guest、Target Runtime 和原生工程分别由 `products/`、`crates/targets/` 管理，手写内容不进入 `dist/`。

浏览器是 `tela-target-webview` 的一个开发态壳：页面先加载固定的 Target host，再以
`cache: "no-store"` 请求 `/tela-dev/latest.json` 和 `.tela` archive，校验后实例化其中的
`app.wasm`。产品页面只走 WGPU；`rawgpu.html` 仍保留为不经过 tela renderer 的浏览器环境诊断页。
desktop 应用 guest、Win32 壳、macOS 壳和浏览器壳共享同一份应用 ABI 与 bundle 协议；Android 使用独立
mobile bundle，iPhone 则静态链接独立移动应用，不进入动态 bundle 协议。

## 非需求（明确不在基座范围）

- 不内置任何业务组件（按钮/输入框/表格等由上层组件套件实现）。
- 不内置业务专属视觉特效。
- 不提供网络、存储、路由、表单校验能力。
- 不绑定任何业务项目。

## 写作原则

- 证据优先：现状部分必须与源码、manifest、测试一致；无法证明的陈述不写或标注为"目标/待定"。
- 显式 > 潜规则：规则写成条文，标注强制状态（✅已强制 / 🔧待强制 / 👁流程检查）。
- 失真即改：代码变化让文档失实时，在同一变更里更新文档，不留下悬空引用。
- 中文写作，代码与命令除外。
