# device/009-getBatteryLevel

> **状态：📐 设计已确认，MVP 定稿。** `std.device.getBatteryLevel`：电量水平拉取。体系规则见 [000-宿主桥总览](../000-宿主桥总览.md)，通用类型见 [通用模型](../通用模型/README.md)。

## 1. 结论

`std.device.getBatteryLevel` 让页面查询当前**电量水平**（0.0-1.0）。用于低电量告警、省电策略提示、电量百分比展示。

**本桥只回答"还有多少电"**；是否充电是独立原子桥 [010-getBatteryCharging](010-getBatteryCharging.md)，按需组合。**不含"电量变化订阅"**（预留，见 [reserved](../reserved.md)）。

## 2. 请求

```text
BridgeRequest::BatteryLevel { version: VersionPolicy }
```

首期版本：`std.device.getBatteryLevel` v1（`major=1, minor=0, patch=0`）。

## 3. 响应

```text
BridgeResult::BatteryLevel(BatteryLevelInfo {
    level: f32,   // 电量水平 0.0 - 1.0（0 = 空，1 = 满）
})
```

| 字段 | 语义 |
|---|---|
| `level` | 电量水平，归一化 0.0-1.0 闭区间；各端原始百分比（如 87%）除以 100 归一化；未知时回 `0.0`（MVP 各端均可读取真实值） |

### 语义边界

- **归一化**：`level` 恒为 0.0-1.0 闭区间，不做百分比整数表达；guest 需要百分比时自行换算。
- **只读快照**：不提供电量变化订阅（MVP 纯拉取）；持续监听预留（[reserved](../reserved.md)）。
- **充电状态**：本桥不含充电信息；"是否在充电"用 [010-getBatteryCharging](010-getBatteryCharging.md)。

## 4. Host 实现

| Target | 来源 | 时序 |
|---|---|---|
| webview | `navigator.getBattery().level` | **异步**（Promise），完成时回投 |
| win32 | WinRT `Windows.System.Power.PowerManager.RemainingChargePercent / 100` | 本地读，同轮回投 |
| macOS | IOKit power source `kIOPSCurrentCapacityKey` / `kIOPSMaxCapacityKey` | 本地读，同轮回投 |
| android | `BatteryManager.BATTERY_PROPERTY_CAPACITY / 100` | 本地读，同轮回投 |
| iOS | `UIDevice.current.batteryLevel`（需 `batteryMonitoringEnabled = true`） | 本地读，同轮回投 |

时序差异（webview 异步、其余同步）是 host 实现自由度（[000](../000-宿主桥总览.md) §4），guest 一律按异步 callback 消费。

## 5. 版本历史

| 版本 | 变更 |
|---|---|
| 1.0.0 | 首版：`level` |

## 6. 验收

- 五端归一化：`level` 恒在 0.0-1.0。
- webview 异步路径：Promise 完成后回投，响应可达（跨帧允许）。
- 未知值回退语义与文档一致。
