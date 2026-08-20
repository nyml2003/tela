# device/011-getNetworkOnline

> **状态：📐 设计已确认，MVP 定稿。** `std.device.getNetworkOnline`：在线状态拉取。体系规则见 [000-宿主桥总览](../000-宿主桥总览.md)，通用类型见 [通用模型](../通用模型/README.md)。

## 1. 结论

`std.device.getNetworkOnline` 让页面查询当前**是否在线**。用于在线/离线提示、离线降级策略（本地缓存优先）、重试决策。

**本桥只回答"有没有网络连接"**；连接类型是独立原子桥 [012-getNetworkKind](012-getNetworkKind.md)，按需组合。**不含"状态变化订阅"**（预留，见 [reserved](../reserved.md)）。

## 2. 请求

```text
BridgeRequest::NetworkOnline { version: VersionPolicy }
```

首期版本：`std.device.getNetworkOnline` v1（`major=1, minor=0, patch=0`）。

## 3. 响应

```text
BridgeResult::NetworkOnline(NetworkOnlineInfo {
    online: bool,   // 是否在线（有可达的网络连接）
})
```

| 字段 | 语义 |
|---|---|
| `online` | 是否在线：有可用网络连接（不保证互联网可达，只是链路层/接口级在线）。各端定义见 §4 |

### 语义边界

- **online ≠ 互联网可达**：`online` 是连接存在性（有网卡/接口/无线连接），不是 `example.com` 可达性；guest 需要"能上网"语义时自行探测（如请求业务接口）。
- **连接类型**：本桥不含类型信息；"是什么连接"用 [012-getNetworkKind](012-getNetworkKind.md)。
- **只读快照**：不提供连接变化订阅（MVP 纯拉取）；持续监听预留（[reserved](../reserved.md)）。

## 4. Host 实现

| Target | 来源 | 时序 |
|---|---|---|
| webview | `navigator.onLine` | 本地读，同轮回投 |
| win32 | WinRT `NetworkInformation.GetInternetConnectionProfile().IsInternetAvailable` | 本地读，同轮回投 |
| macOS | `SCNetworkReachabilityCreateWithName` + flags（`kSCNetworkReachabilityFlagsReachable`） | 本地读，同轮回投 |
| android | `ConnectivityManager.activeNetwork` + `NET_CAPABILITY_INTERNET` | 本地读，同轮回投 |
| iOS | `NWPathMonitor` 当前 path（`NWPath.status`） | 本地读，同轮回投 |

## 5. 版本历史

| 版本 | 变更 |
|---|---|
| 1.0.0 | 首版：`online` |

## 6. 验收

- 在线/离线两态各断言。
- 五端归一化：`online` 语义一致。
