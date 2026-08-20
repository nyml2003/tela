# 通用模型（宿主桥共享类型）

> **状态：📐 设计已确认，MVP 定稿。** 本文是宿主桥**跨桥共享类型**的单一事实来源：`Version` / `VersionPolicy` / `CapabilityId` / `BridgeError` / 协议信封（`Topic` 预留，见 §5）。各桥文档只保留桥专属语义并引用本文，不重复定义。

## 1. 定位与原则

- 通用模型 = 被两个及以上桥共享的类型与规则；桥专属类型留在各自文档。
- **统一外部契约**：所有能力（`std` 公开规范、端侧桥、业务桥）对 guest 一律是"宿主提供的外部能力"，经同一套发现/版本/请求通道访问；guest 统一视为外部实现者（按 ABI 规范接入，无特权路径）。
- **统一字节通道**：信封携带 `CapabilityId` + 载荷字节；各桥载荷格式由契约文档定义（`tela-bridge` 提供编解码辅助，协议不承载类型化枚举）。
- **显式语义**：所有请求必须携带 `VersionPolicy`，禁止隐式默认（见 §3）。
- **原子性**：每桥单一职责、独立可查；复杂用法由 guest 组合（见 [000](../000-宿主桥总览.md) §3、§5）。
- 类型归属 `crates/delivery/bridge/`（package `tela-bridge`），零平台依赖。

## 2. Version

```text
Version { major: u32, minor: u32, patch: u32 }
```

- **通用三元组**：桥能力版本、应用版本（`app_version`）、交付构建（`bundle_build`）三类场景复用同一类型，不重复定义。
- **比较**：字典序（major 优先，其次 minor、patch）；`Ord` 直接派生。
- **bump 规则**由各场景文档定义（桥能力逐版本记录语义；应用/交付见 [device/001-getAppName](../device/001-getAppName.md) 组文档）。
- 预发布/渠道后缀（如 `-beta.1`）不在三元组内表达；需要时由场景文档另行设计。

## 3. VersionPolicy

```text
VersionPolicy =
    | Latest                          // 用最新实现
    | Exact(Version)                  // 指定精确版本
    | Range { lower: Option<Version>, // 版本区间，闭区间 [lower, upper]
              upper: Option<Version> }
```

| 策略 | 匹配条件 | 说明 |
|---|---|---|
| `Latest` | host 有实现即可 | host 返回自身实现版本（`hit_version`），guest 以它为准，不做隐式语义假设 |
| `Exact(v)` | `host == v` | 组件逐项完全相等，语义最严格 |
| `Range { lower, upper }` | `lower_eff <= host <= upper_eff` | `lower: None` 语义 = `0.0.0`；`upper: None` 语义 = `255.255.255`（当前封顶值，暂定可调） |

- 常规选择："至少 v" 写作 `Range { lower: Some(v), upper: None }`。
- 策略不满足 → `BridgeError::VersionMismatch { policy, available }`（§6），host 回显实际版本，不做模糊降级。
- 所有 `BridgeRequest` 携带 `version: VersionPolicy`。

## 4. CapabilityId 与分组

```text
CapabilityScope = Std | Named(String)
CapabilityId { scope: CapabilityScope, group: String, name: String }
// Display："std.device.getBatteryLevel" / "shop.cart.getCount"
```

- **三级能力 ID**：`scope`（`std` 或端侧/业务命名 scope）+ `group`（功能分组）+ `name`（原子能力名）。`std` 是 tela 发布的跨端公开规范；`Named` scope 由实现者注册（端侧如 `web`、`android`；业务如宿主自定义 `shop`）。
- 类型化常量：`CapabilityId::std("device", "getBatteryLevel")`、`CapabilityId::named("shop", "cart", "getCount")`，wire 上序列化三字段，零解析。
- **执行统一**：std / 端侧 / 业务能力经同一注册表执行（[000](../000-宿主桥总览.md) §8），无机制差异。
- 预留能力（`pubsub` / `route` / `storage` / `user` 等命名空间）**不注册**，canIUse 对未注册能力回 `UnknownCapability`（见 [000](../000-宿主桥总览.md) §2.4）。

## 5. Topic（预留）

```text
Topic(String)   // 消息总线主题，MVP 不注册
```

MVP 只做只读桥，消息总线（`subscribe` / `unsubscribe` / `publish`）整体后置（[reserved](../reserved.md) §3）。Topic 类型与 `BridgeEvent::Message` 随总线回归时启用；回归语义记录于 [reserved](../reserved.md)。

## 6. BridgeError（全集）

| 错误 | 含义 |
|---|---|
| `UnknownCapability` | 契约层不认识的能力（预留/未知 group 或 name） |
| `VersionMismatch { policy, available }` | `VersionPolicy` 策略不满足（§3） |
| `PermissionDenied` | 权限拒绝（position.getCoordinates 权限流，见 [position/001-getCoordinates](../position/001-getCoordinates.md)） |
| `KeyNotFound` | key 不存在（config.getConfig 未命中，见 [config/001-getConfig](../config/001-getConfig.md)） |
| `Timeout` | 宿主执行超时（防御性保留，MVP host 可不实现） |

## 7. 协议信封

```text
BridgeRequest = {
    request_id: u64,             // guest 分配，响应关联
    version: VersionPolicy,      // 每请求显式携带
    capability: CapabilityId,    // 任意 scope 的能力（std / 端侧 / 业务）
    payload: Vec<u8>,            // 能力定义的请求载荷字节
}
BridgeResult  = Ok(Vec<u8>)      // 能力定义的响应载荷字节
              | Err(BridgeError) // 协议级错误
BridgeEvent   = Response { request_id: u64, result: BridgeResult }
              // Message { topic, payload } 随消息总线预留（见 [reserved](../reserved.md)）
```

- **统一字节通道**：信封不承载类型化枚举；各桥载荷格式由契约文档定义（`tela-bridge::payload` 提供编解码辅助函数，guest/host 在调用点保持类型安全）。
- **协议级错误结构化**：`UnknownCapability` / `VersionMismatch` / `PermissionDenied` / `KeyNotFound` / `Timeout` 由信封结构化携带，guest 可区分"能力缺失/版本不满足/超时"与业务结果。
- **异步语义**：`Response` 可在任意帧到达（[000](../000-宿主桥总览.md) §4）；guest 以 `request_id` 关联，host 对本地能力应同轮回投。
- `Message`（pubsub 推送路径，无 request_id）为预留变体，MVP 不编解码（随 [reserved](../reserved.md) §3 总线回归启用）。
- 编解码：postcard + magic `TLBR` + 自持 packet version（风格同 `tela-app-abi`）；请求队列为多包流（`decode_request_stream`）。
- 传输通道（request 队列、`tela_app_bridge_dispatch`）见 [000](../000-宿主桥总览.md) §7。
