# device/012-getNetworkKind

> **状态：📐 设计已确认，MVP 定稿。** `std.device.getNetworkKind`：连接类型拉取。体系规则见 [000-宿主桥总览](../000-宿主桥总览.md)，通用类型见 [通用模型](../通用模型/README.md)。

## 1. 结论

`std.device.getNetworkKind` 让页面查询当前**连接类型**（Wi-Fi / 蜂窝 / 以太网）。用于数据用量警示（蜂窝流量）、质量提示、加载策略分级。

**本桥只回答"什么类型的连接"**；是否在线是独立原子桥 [011-getNetworkOnline](011-getNetworkOnline.md)，按需组合。**不含"状态变化订阅"**（预留，见 [reserved](../reserved.md)）。

## 2. 请求

```text
BridgeRequest::NetworkKind { version: VersionPolicy }
```

首期版本：`std.device.getNetworkKind` v1（`major=1, minor=0, patch=0`）。

## 3. 响应

```text
BridgeResult::NetworkKind(NetworkKindInfo {
    kind: NetworkKind,   // 连接类型
})

NetworkKind = Wifi | Cellular | Ethernet | Unknown
```

| 字段 | 语义 |
|---|---|
| `kind` | 连接类型：`Wifi` / `Cellular` / `Ethernet` / `Unknown`；离线或无连接时回 `Unknown`。跨端归一化：各端平台枚举映射到统一四值（见 §4） |

### 语义边界

- **kind 归一化**：平台枚举（webview `effectiveType`、android `TYPE_WIFI`/`TYPE_MOBILE`、iOS `Cellular`/`WiFi`、WinRT `ConnectionProfile`、macOS `SCNetworkReachability` flags）映射为统一四值；映射不明确的回 `Unknown`。
- **离线语义**：无连接时回 `Unknown`；"是否在线"用 [011-getNetworkOnline](011-getNetworkOnline.md)。
- **只读快照**：不提供连接变化订阅（MVP 纯拉取）。

## 4. Host 实现

| Target | 来源 | 时序 |
|---|---|---|
| webview | `navigator.connection.effectiveType`（映射：`wifi`/`ethernet` → 对应值，`2g/3g/4g/5g` → Cellular，未知 → Unknown） | 本地读，同轮回投 |
| win32 | WinRT `NetworkInformation.GetInternetConnectionProfile().NetworkAdapter.IanaInterfaceType`（映射：6=Ethernet、71/243=Wifi、其他→Unknown） | 本地读，同轮回投 |
| macOS | `SCDynamicStore` 或默认路由接口类型（`en*`=Wifi、`en*` 以太网经接口标志区分，无法区分 → Unknown） | 本地读，同轮回投 |
| android | `ConnectivityManager.getNetworkCapabilities`（`TRANSPORT_WIFI`/`TRANSPORT_CELLULAR`/`TRANSPORT_ETHERNET`） | 本地读，同轮回投 |
| iOS | `NWPathMonitor` 当前 path（`usesInterfaceType(.wifi/.cellular/.wiredEthernet)`） | 本地读，同轮回投 |

## 5. 版本历史

| 版本 | 变更 |
|---|---|
| 1.0.0 | 首版：`kind` 四值 |

## 6. 验收

- 五端归一化：`kind` 语义一致；映射不明确的端回 `Unknown` 而非猜测。
- 各类型值与 `online` 组合断言（`Cellular` 时 `online=true` 等）。
