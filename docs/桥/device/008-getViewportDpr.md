# device/008-getViewportDpr

> **状态：📐 设计已确认，MVP 定稿。** `std.device.getViewportDpr`：设备像素比的拉取快照。体系规则见 [000-宿主桥总览](../000-宿主桥总览.md)，通用类型见 [通用模型](../通用模型/README.md)。

## 1. 结论

`std.device.getViewportDpr` 让页面主动读取当前**设备像素比**（device pixel ratio）。它与 `AppEvent::Viewport`（host 推送、布局输入）**同一数据源**，本桥是拉取快照。

**本桥只回答"逻辑像素与物理像素的比例"**；内容区尺寸是独立原子桥 [007-getViewportSize](007-getViewportSize.md)，按需组合（物理像素 = 逻辑尺寸 × dpr）。

## 2. 请求

```text
BridgeRequest::ViewportDpr { version: VersionPolicy }
```

首期版本：`std.device.getViewportDpr` v1（`major=1, minor=0, patch=0`）。

## 3. 响应

```text
BridgeResult::ViewportDpr(ViewportDprInfo {
    dpr: f32,   // 设备像素比；物理像素 = 逻辑像素 × dpr
})
```

| 字段 | 语义 |
|---|---|
| `dpr` | 设备像素比；与最近一次 `AppEvent::Viewport` 推送的 DPR 一致 |

### 语义边界

- **同源快照**：与 `AppEvent::Viewport` 推送同源；DPI 变化（窗口跨屏移动、系统缩放变更）后重复查询返回最新值。
- **单独消费场景**：图标/位图资源选择（@1x/@2x/@3x）、绘制精度调整——只关心 dpr，不需要尺寸，不必拉全量 viewport。
- **物理像素换算**：需要物理尺寸时组合 `viewportSize`（[007-getViewportSize](007-getViewportSize.md)）。

## 4. Host 实现

本桥为本地能力，**应同一轮回投**。与最近一次 `AppEvent::Viewport` 推送同源。

| Target | 来源 |
|---|---|
| webview | `window.devicePixelRatio` |
| win32 | DPR 经 `GetDpiForWindow` 计算 |
| macOS | backing scale factor（`convertToBacking` 比值） |
| android | `density` |
| iOS | `scale` |

## 5. 版本历史

| 版本 | 变更 |
|---|---|
| 1.0.0 | 首版：`dpr` |

## 6. 验收

- 拉取值与最近一次 `AppEvent::Viewport` 推送一致（同源快照）。
- DPR 换算正确：物理尺寸 = 逻辑尺寸 × dpr（组合 `viewportSize` 断言）。
- 五端归一化，无缺省假值。
