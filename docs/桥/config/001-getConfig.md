# config/001-getConfig

> **状态：📐 设计已确认，MVP 定稿。** `std.config.getConfig`：按 key 拉取配置。体系规则见 [000-宿主桥总览](../000-宿主桥总览.md)，通用类型见 [通用模型](../通用模型/README.md)。

## 1. 结论

`std.config.getConfig` 让页面按 key 读取**一个配置值**（如主题、特性开关、服务地址）。配置数据源是 host 的实现细节（静态表、bundle 清单、宿主平台配置，未来可为远程下发），guest 不感知来源。**配置是只读的**：不提供写入/更新订阅（MVP 纯拉取）。

**本桥只回答"key 对应的值是什么"**；key 不存在回 `BridgeError::KeyNotFound`（错误路径），不做 Option 兜底——页面把"没有配置"视为明确的失败信号。

## 2. 请求

```text
BridgeRequest::ConfigGet {
    key: String,           // 配置键，见下
    version: VersionPolicy,
}
```

| 字段 | 语义 |
|---|---|
| `key` | 配置键：点分字符串（如 `"app.theme"`、`"features.map"`），非空、UTF-8、≤256 字节；非法 key（空串/超长/非 UTF-8）拒绝 |

首期版本：`std.config.getConfig` v1（`major=1, minor=0, patch=0`）。

## 3. 响应

```text
BridgeResult::ConfigGet(ConfigValue {
    value: String,   // JSON 文本
})
```

| 字段 | 语义 |
|---|---|
| `value` | 配置值：**JSON 文本**（`"{}"`、`"true"`、`"\"dark\""` 等），guest 自行解析；key 存在但值为空对象/空数组时仍返回对应 JSON |

错误路径：

| 错误 | 触发 |
|---|---|
| `KeyNotFound` | key 不存在（host 未声明该 key） |
| `Timeout` | 远程数据源超时（防御性，MVP host 可不实现） |

### 语义边界

- **未命中是错误**：key 不存在回 `BridgeError::KeyNotFound`，不返回 Option；SDK 兜底行为 = 直接向调用方回 Err。
- **JSON 文本**：值是 JSON 文本字符串，不是任意文本；guest 解析失败视为配置错误（host 保证合法 JSON，MVP 不做 schema 校验）。
- **只读**：MVP 每次查询返回当前值；不提供配置变更订阅（[reserved](../reserved.md) 预留）。
- **幂等**：同 key 重复查询返回一致结果。
- **数据源可远程**：host 可从静态表或远程下发取值；MVP 实现为静态表，契约不承诺同轮回投（远程可能跨帧）。

## 4. Host 实现

| Target | 来源 | 时序 |
|---|---|---|
| 全部 | MVP：构建期注入的静态配置表（JSON 文本）；key 集合由各 Target 声明，未声明 key 回 `KeyNotFound`；webview 可并入 bundle 索引或构建常量 | 静态表 = 本地读，同轮回投；未来远程数据源 = 异步回投 |

契约允许远程（异步），MVP 实现静态表（同步）；guest 一律按异步 callback 消费，不区分。

## 5. 版本历史

| 版本 | 变更 |
|---|---|
| 1.0.0 | 首版：`key` → `ConfigValue { value: String }`；未命中回 `KeyNotFound` |

## 6. 验收

- 命中 key 返回对应 JSON 文本。
- 未命中 key 回 `BridgeError::KeyNotFound`。
- 非法 key（空串/超长/非 UTF-8）拒绝。
- 幂等：同 key 重复查询一致。
- 静态表实现同轮回投断言。
