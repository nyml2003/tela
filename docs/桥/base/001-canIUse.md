# base/001-canIUse

> **状态：📐 设计已确认，MVP 定稿。** 宿主桥的元能力：能力发现与版本协商。体系规则见 [000-宿主桥总览](../000-宿主桥总览.md)，通用类型见 [通用模型](../通用模型/README.md)。

## 1. 结论

`std.base.canIUse` 是 guest 查询宿主能力的唯一入口，**必须常驻**（任何桥实现都包含它）。查询返回 `Result`：**成功即代表能力存在且版本策略命中**，失败由错误区分原因（不认识 / 版本不满足 / 超时），因此没有 `supported` 布尔字段。guest 不做隐式猜测，一切以 canIUse 的显式响应为准。

## 2. 请求

```text
BridgeRequest::CanIUse {
    capability: CapabilityId,    // { group, name }，如 "std.device.getBatteryLevel"
    version: VersionPolicy,      // Latest | Exact | Range（见 通用模型 §3）
}
```

| 字段 | 语义 |
|---|---|
| `capability` | 目标能力；`std` 能力用类型化常量（`CapabilityId::std("device", "battery")`）。**能力内子特性也建模为独立能力**（如未来 `std.position.watch`），走同一套查询机制 |
| `version` | 期望语义的 `VersionPolicy`：`Latest` = 最新实现；`Exact(v)` = 精确版本；`Range { lower, upper }` = 闭区间（`lower: None` = 0.0.0，`upper: None` = 255.255.255 封顶），匹配规则见 §4 |

## 3. 响应

```text
BridgeResult::CanIUse(hit_version: Version)
```

成功即"能力存在 + `VersionPolicy` 命中"，返回命中的 host 实际实现版本；`Latest` 策略下它就是"最新是多少"的答案。**没有 `supported` 字段**：能力不存在或不满足策略一律走错误路径。

错误路径：

| 错误 | 触发 |
|---|---|
| `UnknownCapability` | 预留/未知 group 或 name（见 [000](../000-宿主桥总览.md) §2.3 预留清单） |
| `VersionMismatch { policy, available }` | host 实现的 canIUse 版本不满足 guest 的 `VersionPolicy`（发现机制自身语义保护） |

## 4. 版本匹配规则

匹配按 `VersionPolicy` 三策略执行，host 实现版本记为 `host`（字典序比较，major 优先），命中则返回 `host`：

```text
Latest              ->  命中 = host 存在
Exact(v)            ->  命中 = (host == v)
Range { l, u }      ->  l_eff = l 或 0.0.0；u_eff = u 或 255.255.255（封顶）
                        命中 = (l_eff <= host <= u_eff)
```

- `Exact` 要求组件逐项完全相等，语义最严格；"至少 v" 用 `Range { lower: Some(v), upper: None }` 表达。
- 不满足时 host 回 `VersionMismatch { policy, available }`，`available` 是 host 实际版本，guest 据此调整策略或降级。
- `Latest` 不保证语义版本：guest 必须消费返回的 `hit_version` 后按其记录的版本语义行事。

## 5. 批量查询（子请求）

```text
BridgeRequest::ListCapabilities  ->  BridgeResult::ListCapabilities(Vec<CapabilityEntry>)
CapabilityEntry { capability: CapabilityId, hit_version: Version }
```

guest 启动时一次建立版本表，不必逐能力查询；子特性（独立 `CapabilityId`）也出现在表中，guest 按 group 过滤。

## 6. Host 实现

- canIUse 的 host 实现 = **构建期静态表**，每个 Target 生成自己的表；MVP 十一能力全部实现（见 [000](../000-宿主桥总览.md) §2.2）。静态表已注册的能力必须响应成功，不允许报 unknown。
- host 实现的 canIUse 版本（`std.base.canIUse` 的命中版本）不满足 guest 的 `VersionPolicy` 时回 `VersionMismatch`，保证发现机制本身语义稳定。
- 本桥为本地能力，**应同一轮回投**（[000](../000-宿主桥总览.md) §4）。

## 7. 版本历史

| 版本 | 变更 |
|---|---|
| 1.0.0 | 首版：`CanIUse` / `ListCapabilities` |

## 8. 验收

- 查询三策略路径：`Latest` / `Exact`（相等与不等）/ `Range` 边界（`lower: None` = 0.0.0、`upper: None` = 255.255.255、闭区间端点包含）。
- `UnknownCapability`（预留/未知 group 或 name）路径：未知能力回错误，已注册能力永不报 unknown。
- 成功响应无 `supported` 字段：能力存在且命中即成功，其余全部走错误。
- `ListCapabilities` 与逐能力查询结果一致；子特性（独立 `CapabilityId`）在表中按 group 可过滤。
- 静态表一致性：注册即实现（见 [000](../000-宿主桥总览.md) §11）。
- 同轮回投断言（本地能力，mock 无延迟）。
