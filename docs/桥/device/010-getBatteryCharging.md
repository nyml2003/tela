# device/010-getBatteryCharging

> **状态：📐 设计已确认，MVP 定稿。** `std.device.getBatteryCharging`：充电状态拉取。体系规则见 [000-宿主桥总览](../000-宿主桥总览.md)，通用类型见 [通用模型](../通用模型/README.md)。

## 1. 结论

`std.device.getBatteryCharging` 让页面查询当前**是否在充电**。用于充电状态展示、充电中行为策略（如"充电时同步"）。

**本桥只回答"是否在充电"**；电量水平是独立原子桥 [009-getBatteryLevel](009-getBatteryLevel.md)，按需组合。**不含"充电状态变化订阅"**（预留，见 [reserved](../reserved.md)）。

## 2. 请求

```text
BridgeRequest::BatteryCharging { version: VersionPolicy }
```

首期版本：`std.device.getBatteryCharging` v1（`major=1, minor=0, patch=0`）。

## 3. 响应

```text
BridgeResult::BatteryCharging(BatteryChargingInfo {
    charging: bool,   // 是否充电中（含插电已满）
})
```

| 字段 | 语义 |
|---|---|
| `charging` | 是否在充电（含插电已满）；未知时回 `false` |

### 语义边界

- **只读快照**：不提供充电状态变化订阅（MVP 纯拉取）；持续监听预留（[reserved](../reserved.md)）。
- **电量水平**：本桥不含电量；"还有多少电"用 [009-getBatteryLevel](009-getBatteryLevel.md)。
- **插电已满**：充电中且满电仍回 `true`（语义为"连接了电源"）。

## 4. Host 实现

| Target | 来源 | 时序 |
|---|---|---|
| webview | `navigator.getBattery().charging` | **异步**（Promise），完成时回投 |
| win32 | WinRT `Windows.System.Power.PowerManager.PowerStatus` | 本地读，同轮回投 |
| macOS | IOKit power source `kIOPSIsChargingKey` / `kIOPSPowerSourceStateKey` | 本地读，同轮回投 |
| android | `ACTION_BATTERY_CHANGED` status（`BATTERY_STATUS_CHARGING` / `FULL`） | 本地读，同轮回投 |
| iOS | `UIDevice.current.batteryState`（需 `batteryMonitoringEnabled = true`） | 本地读，同轮回投 |

时序差异是 host 实现自由度（[000](../000-宿主桥总览.md) §4），guest 一律按异步 callback 消费。

## 5. 版本历史

| 版本 | 变更 |
|---|---|
| 1.0.0 | 首版：`charging` |

## 6. 验收

- 五端归一化：`charging` 语义一致（含插电已满回 `true`）。
- webview 异步路径：Promise 完成后回投，响应可达（跨帧允许）。
- 未知值回退语义与文档一致。
