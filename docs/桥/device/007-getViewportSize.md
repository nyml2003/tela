# device/007-getViewportSize

> **状态：📐 设计已确认，MVP 定稿。** `std.device.getViewportSize`：内容区（视口）逻辑尺寸的拉取快照。体系规则见 [000-宿主桥总览](../000-宿主桥总览.md)，通用类型见 [通用模型](../通用模型/README.md)。

## 1. 结论

`std.device.getViewportSize` 让页面主动读取当前**内容区**（视口）的逻辑尺寸。它与 `AppEvent::Viewport`（host 推送、布局输入）**同一数据源**，本桥是拉取快照——页面在渲染帧外需要当前值时主动查询。

**本桥只回答"内容区多大"**；设备像素比是独立原子桥 [008-getViewportDpr](008-getViewportDpr.md)，按需组合。宽度与高度是绑定成对消费（布局需要完整尺寸），合于本桥。

## 2. 请求

```text
BridgeRequest::ViewportSize { version: VersionPolicy }
```

首期版本：`std.device.getViewportSize` v1（`major=1, minor=0, patch=0`）。

## 3. 响应

```text
BridgeResult::ViewportSize(ViewportSizeInfo {
    width: u32,   // 内容区逻辑宽（已扣安全区，与 AppEvent::Viewport 同源）
    height: u32,  // 内容区逻辑高
})
```

| 字段 | 语义 |
|---|---|
| `width` / `height` | 内容区逻辑尺寸（CSS point 等价物）；**已扣除安全区**，与最近一次 `AppEvent::Viewport` 推送值一致 |

### 语义边界

- **双通道分工**：`AppEvent::Viewport` 是推送（壳每次变化下发，布局引擎的输入）；`std.device.getViewportSize` 是拉取（页面主动读取当前快照）。两者同源，不互替——布局消费推送事件，页面在帧外（如初始化、事件回调、诊断）用桥查询。
- **值会变化**：resize/DPI 变化/旋转时内容区改变；桥每次查询返回最新快照，不缓存语义。
- **安全区不在本能力**：壳在 viewport 归一化时扣除安全区；原始安全区值查询另说（[reserved](../reserved.md)）。
- **需要物理像素**：逻辑尺寸 × `viewportDpr`（[008-getViewportDpr](008-getViewportDpr.md)），guest 自行组合。

## 4. Host 实现

本桥为本地能力，**应同一轮回投**。与最近一次 `AppEvent::Viewport` 推送同源（host 缓存同源快照）。

| Target | 来源 |
|---|---|
| webview | canvas 的 CSS 逻辑尺寸 |
| win32 | `GetClientRect`（客户区逻辑尺寸） |
| macOS | `NSView.bounds` 逻辑尺寸 |
| android | surface 逻辑尺寸 |
| iOS | view bounds 逻辑尺寸 |

## 5. 版本历史

| 版本 | 变更 |
|---|---|
| 1.0.0 | 首版：`width` / `height` |

## 6. 验收

- 拉取值与最近一次 `AppEvent::Viewport` 推送一致（同源快照）。
- resize 后重复查询返回最新值。
- 五端字段归一化，无缺省假值。
