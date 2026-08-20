# position/001-getCoordinates

> **状态：📐 设计已确认，MVP 定稿。** `std.position.getCoordinates`：位置信息单次拉取。体系规则见 [000-宿主桥总览](../000-宿主桥总览.md)，通用类型见 [通用模型](../通用模型/README.md)。

## 1. 结论

`std.position.getCoordinates` 让页面查询当前**经纬度坐标**（含坐标编码 datum、水平精度与定位时刻）。取值流程为缓存优先 → 系统获取 → 系统权限流。**异步语义**：系统获取/权限流可能跨帧回投。**不含连续定位**（watch 预留，见 [reserved](../reserved.md)）。

## 2. 请求

```text
BridgeRequest::GetCoordinates { version: VersionPolicy }
```

首期版本：`std.position.getCoordinates` v1（`major=1, minor=0, patch=0`）。

## 3. 响应

```text
BridgeResult::GetCoordinates(Coordinates {
    latitude: f64,          // 纬度（度，datum 坐标系）
    longitude: f64,         // 经度（度，datum 坐标系）
    accuracy_meters: f32,   // 水平精度（米）
    timestamp_millis: u64,  // 定位时刻（unix 毫秒）
    datum: Datum,           // 坐标编码/基准，必须声明
})

Datum = WGS84 | GCJ02 | BD09
```

| 字段 | 语义 |
|---|---|
| `latitude` / `longitude` | 位置坐标，单位为度，**坐标系由 `datum` 声明**；guest 必须按 datum 解释坐标，不得假设 WGS84 |
| `accuracy_meters` | 水平精度（米），越小越准；全端定位 API 均提供，必填 |
| `timestamp_millis` | 定位产生的时刻（unix 毫秒，wall clock），非查询时刻 |
| `datum` | 坐标编码/基准，**必须声明**：`WGS84`（GPS 国际标准）、`GCJ02`（国测局加密）、`BD09`（百度）。所有端原生 API 均返回 WGS84；GCJ-02/BD-09 仅当 host 显式做坐标转换时使用 |

### 语义边界

- **datum 必须声明的原因**：把 GCJ-02 坐标当 WGS84 叠加地图是经典偏移 bug；guest 拿到坐标后按 datum 决定是否转换。MVP 各端均返回 `WGS84`，枚举保留扩展（host 显式转换时才使用其他变体）。
- **缓存语义**：host 维护最近一次定位缓存；请求时先读缓存，命中则直接返回（不触发系统获取）。缓存时效由 host 实现（MVP：恒取最新缓存）。
- **权限语义**：缓存未命中且系统有权限要求时，host 走各端权限流程；用户拒绝回 `BridgeError::PermissionDenied`（[通用模型](../通用模型/README.md) §6）。
- **异步语义**：系统获取与权限流为异步路径，响应可跨帧回投（[000](../000-宿主桥总览.md) §4）；guest 一律按 callback 消费，不假设同帧。
- **首期范围**：仅单次拉取；不含 altitude/speed/heading。连续定位（watch）预留（[reserved](../reserved.md)）。

## 4. 取值流程

```text
页面请求 get
  -> host 读缓存定位；命中则返回（含缓存时刻）
  -> 未命中：检查系统权限
     - 无权限要求：直接获取（异步）
     - 有权限要求：走各端系统权限流程（见 §5）
  -> 用户拒绝：BridgeError::PermissionDenied
  -> 获取成功：返回 Coordinates，并更新缓存
```

## 5. Host 实现

| Target | 来源 | 权限流程 | 时序 |
|---|---|---|---|
| webview | `navigator.geolocation.getCurrentPosition` | 浏览器权限弹窗 | 异步 |
| win32 | UWP `Windows.Devices.Geolocation`（`Geolocator.GetGeolocationAsync`） | UWP 应用权限设置 | 异步 |
| macOS | CoreLocation（`CLLocationManager`） | TCC 系统弹窗（需 `NSLocationUsageDescription`） | 异步 |
| android | 系统定位（`FusedLocationProvider` 或 `LocationManager`） | 系统权限弹窗（Manifest 声明定位权限） | 异步 |
| iOS | CoreLocation（`CLLocationManager`） | 系统权限弹窗（Info.plist 声明用途） | 异步 |

各端归一化为统一字段，datum 恒 `WGS84`；host 显式转换时才声明 `GCJ02`/`BD09`。异步完成经 `BridgeDispatcher::complete` 投递回 UI 线程后回投。

## 6. 版本历史

| 版本 | 变更 |
|---|---|
| 1.0.0 | 首版：`Coordinates` 五字段，异步语义 |

## 7. 验收

- 五端字段归一化：全字段必填，datum 恒 `WGS84`（无转换时）。
- 缓存路径：命中缓存时不触发系统获取（可注入 mock 断言）。
- 权限拒绝路径：返回 `BridgeError::PermissionDenied`。
- 异步路径：mock 延迟 provider 下响应跨帧可达，request_id 关联正确。
- 坐标解释：datum 声明与坐标系一致；转换路径（如 GCJ-02）有单元测试覆盖。
