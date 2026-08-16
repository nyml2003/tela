# 025-WebView 开发 SDK 实施目标

> **状态：✅ 开发态 WGPU 路径已实现。** 本文记录浏览器/通用 WebView 壳的实际边界、启动协议、验收方式和明确后置项。它不是 Android/iOS native bridge 设计，也不承诺生产离线分发。

## 1. 目标

浏览器不应维护一份“特供 demo wasm”和另一套输入/渲染协议。`tela-webview-sdk` 将浏览器变成与 Win32/macOS 平行的应用壳：固定壳加载一个经过校验的 `.tela`，实例化其中的 `app.wasm`，向 guest 传入版本化输入包，并将 `UiFrame` 呈现到 canvas。

```text
可编辑源码                         可删除构建产物

crates/tela-demo (app-wasm)    -> dist/tela-dev/tela-demo.tela
crates/tela-webview-sdk        -> dist/tela_webview_sdk.js + _bg.wasm
web/src/webview-sdk            -> dist/assets/tela-web/app.js
web/public/index.html          -> dist/index.html
```

成功标准：在支持 WebGPU 的浏览器/WebView 中，打开根页面会从 `/tela-dev/latest.json` 启动同一份应用 guest；指针、键盘、焦点、受控文本和 IME composition 能穿过 ABI；关闭页面不会遗留 DOM listener、隐藏输入框或 WGPU session。

## 2. 不做什么

- 不在浏览器产品页提供 raster/Canvas2D 回退，不维护 `?backend=raster|wgpu|auto`。
- 不让 JavaScript 手写 archive 校验、postcard event 编码或 `UiFrame` 解码。
- 不将 WebView 抽象成 Android/iOS/Win32 共用的大 `Host` trait。
- 不使用 CacheStorage/IndexedDB 保存 archive，不实现浏览器离线回退、热更新、轮询或后台下载。
- 不由 `ops` 或测试命令自动拉起 Chromium。真实浏览器视觉验收由开发者明确发起。

`rawgpu.html` 仍可用，但它只回读一个原生 WebGPU 三角形，用于区分环境故障与 tela WGPU 故障，不是页面后端。

## 3. 分层与所有权

```text
index.html / web/src/main.ts
  -> startTelaWebview({ canvas, bundleIndex })
      -> web/src/webview-sdk/
          DOM fetch / URL、PointerEvent、KeyboardEvent、textarea IME、DPR、生命周期
      -> crates/tela-webview-sdk/
          bundle / ABI 校验、event codec、UiFrame decode、WGPU surface / present
      -> app.wasm（archive 内的 tela-demo guest）
          应用状态、布局、焦点图、键盘意图、键位表、UiFrame
```

所有权规则：

- `tela-demo` 拥有业务状态、DSL、布局、焦点和键盘意图，不接触 DOM/GPU。
- `tela-app-abi` 拥有跨壳的 `AppEvent`/`AppStatus`/帧 packet；ABI v2 增加 IME composition 与运行时键位表替换事件。
- `tela-webview-sdk` Rust 部分拥有可信边界：索引/archive 校验、哈希、ABI、packet 编解码、frame 解码和 WGPU。
- JavaScript 半边只拥有浏览器事实：fetch、DOM 输入、原生 WebAssembly 实例化、布局视口/DPR、资源释放。
- `web/src/main.ts` 只选择 canvas 与默认 bundle URL；它不是第二个业务 runtime。

`ops check` 通过依赖方向规则禁止 `tela-webview-sdk` 依赖 `tela-demo`。壳可消费 ABI/bundle/renderer，但应用不能被壳反向绑定。

## 4. 启动与可信边界

默认启动入口：

```ts
const session = await startTelaWebview({
  canvas,
  bundleIndex: new URL('/tela-dev/latest.json', window.location.href),
});
```

启动按以下顺序执行：

1. 检查 `navigator.gpu`，没有 WebGPU 时报告明确启动错误。
2. 动态导入 `/tela_webview_sdk.js`，以 `cache: "no-store"` 加载其 `_bg.wasm`。
3. 以 `cache: "no-store"` 请求 index；由 Rust 解析并检查 format、ABI、大小和 URL 约束。
4. 用浏览器 `URL` 语义解析 archive URL，以 `cache: "no-store"` 请求 archive。
5. Rust 重新检查 archive 大小、SHA-256、内部 manifest、路径、资源限制和 ABI，取得 `app.wasm`。
6. JavaScript 实例化 guest，检查必要 export、host/guest ABI，读取初始化后的 frame/status。所有线性内存复制都检查 pointer、长度与 64 MiB 上限。
7. Rust 创建 WGPU canvas surface；同步 CSS logical size 与 DPR backing store，派发首个 viewport，安装输入桥并请求首帧。

开发服务器仅对 `/tela-dev/*` 返回 `Access-Control-Allow-Origin: *`。这允许开发机提供 bundle 给另一台机器上的 WebView，同时不会扩大普通静态页面与 SDK glue 的跨域可读范围。

WebView 不使用 Wasmtime。原生壳的 fuel 和最后有效 archive 缓存仍是原生开发体验的一部分，不能被误写成浏览器能力。

## 5. 输入、焦点和键位表

### 5.1 原始输入与键盘意图

DOM `KeyboardEvent.code` 映射到 USB-HID physical key，Shift/Ctrl/Alt/Meta 保持 modifier bit mask。SDK 不解释快捷键含义；guest 根据自身 `KeymapSnapshot` 将物理键解析为 `KeyboardIntent`，再由 core 的焦点图规约。因此同一键位表可由浏览器、Win32 和 macOS 使用。

`TelaWebviewSession.replaceKeymap(snapshot)` 会把 JSON 快照包装成 ABI v2 `ReplaceKeymapJson`。guest 对 snapshot 校验成功后原子替换；失败不改变旧表。页面还暴露受控的开发控制台入口：

```ts
window.telaReplaceKeymap?.({ /* 完整 keymap snapshot */ });
```

这不是给组件传裸回调，也不是让 UI 元素管理业务键位表。

### 5.2 文本与 IME

每个 session 创建一个隐藏受控 `<textarea>`。guest 的 `AppStatus.input_focused` 决定它是否持有 DOM 焦点；`AppStatus.cursor` 决定 canvas cursor。textarea 的 `input` 生成 `SetInputValue`，focus/blur 生成边沿事件，`compositionstart`/`compositionend` 生成 ABI v2 标记。composition 本身不提交业务数据，最终可见文本仍走受控值，因此候选词、Tab 焦点和取消语义不会混为一谈。

canvas 在非文本模式接收物理键；pointer down 会根据 guest 最新状态在 canvas/textarea 之间同步焦点。关闭、失焦与状态变更都通过同一个同步点收敛，避免“进入新建按钮更新底部提示，移出却未恢复”一类宿主生命周期残留。

## 6. 尺寸、渲染和生命周期

CSS 逻辑 viewport 与 WGPU backing store 有一个单一同步点：`ResizeObserver`、window resize 与 DPR media query 都先更新 `canvas.width/height`，再以逻辑像素向 guest 派发 `Viewport`。pointer/wheel 使用同一份当前逻辑 viewport 做坐标换算。这避免大 canvas 只绘制左上角、DPR 改变后裁切旧 frame 或滚动区错误 clip。

渲染不是永久 rAF 循环：启动、输入、viewport 变更时合并为一次 frame 请求。WGPU surface 返回 `Outdated`、`Lost`、`Timeout` 或 `Occluded` 时，session 以短 retry 重试；不可恢复错误记录包含 Rust `gpu_diagnostics()` 的控制台诊断。

`session.close()` 是唯一关闭入口，且幂等：取消 rAF/timer、停止 `ResizeObserver` 和 DPR listener、卸载全部 DOM listener、移除 textarea、恢复 canvas cursor、释放 Rust WGPU session。`pagehide` 调用它并清除全局开发键位表入口。

## 7. 构建与验收

```bash
ops check
ops build
ops verify bundle
ops serve
```

`ops build` 重建 `dist/`，依次生成 release WebView SDK glue、前端页面和 `.tela`。`ops verify bundle` 在无浏览器环境验证 archive、ABI、guest 初始化与 viewport；`ops verify gpu` 只用于 raw WebGPU 环境诊断。两者都不能替代真实浏览器的视觉验收。

人工验收清单：

- 打开 `ops serve` 输出的根 URL，确认 canvas 非空且铺满设计的客户端视口。
- 改变窗口尺寸、缩放级别或 DPR，确认 backing store、viewport 和点击命中同步更新。
- 点击树、文件列表和工具栏，确认 cursor/hover/focus/状态栏在进入与离开时都恢复。
- 用 Tab、方向键、Enter、Escape 验证 core 默认焦点策略和组件显式规约。
- 使用中文/日文 IME 输入、取消和提交，确认 composition 不提前提交草稿。
- 在控制台调用 `window.telaReplaceKeymap(...)`，确认有效表生效、无效表不破坏旧表。
- 导航或关闭页面后检查无残留 textarea、无重复 listener、无持续 redraw。

## 8. 后置项

| 项目 | 当前选择 | 触发条件 |
| --- | --- | --- |
| 浏览器持久化缓存/离线 | 不做 | 有明确离线开发或生产 PWA 需求时，设计版本失效与完整性策略 |
| raster/Canvas2D 产品回退 | 不做 | 有不支持 WebGPU 的目标设备且可接受独立产品矩阵时 |
| iOS native bridge / Android WebView bridge | 不做 | Android native Target 见 027；只有真实 WebView bridge 需求出现时再设计 |
| 原生完整 IME/剪贴板 | WebView 已有 DOM composition；原生仍后置 | Win32/macOS 有真实文本编辑交付需求时 |
| bundle 资源到 GPU | archive 已校验，渲染通道未接 | 应用开始需要图片/二进制资源渲染时 |
| 浏览器自动视觉测试 | 不做 | 明确选定受支持浏览器与 CI 图形环境后 |

关联文档：[002-架构总览与分层](002-架构总览与分层.md)、[007-绘制与渲染后端](007-绘制与渲染后端.md)、[009-多环境集成](009-多环境集成.md)、[022-构建产物与浏览器宿主目录](022-构建产物与浏览器宿主目录.md)、[023-平台SDK与WASM开发包](023-平台SDK与WASM开发包.md)。
