# device/005-getBundleBuildId

> **状态：📐 设计已确认，MVP 定稿。** `std.device.getBundleBuildId`：交付构建序号拉取。体系规则见 [000-宿主桥总览](../000-宿主桥总览.md)，通用类型见 [通用模型](../通用模型/README.md)。

## 1. 结论

`std.device.getBundleBuildId` 让页面查询**交付构建序号**。用于交付溯源、更新判断（构建序号对比）、诊断信息。

**本桥只回答"当前交付包是第几次构建"**；交付语义版本与应用侧四桥是独立原子桥（[getAppName](001-getAppName.md) / [getAppVersion](002-getAppVersion.md) / [getAppBuildId](003-getAppBuildId.md) / [getBundleVersion](004-getBundleVersion.md)），按需组合。

## 2. 请求

```text
BridgeRequest::GetBundleBuildId { version: VersionPolicy }
```

首期版本：`std.device.getBundleBuildId` v1（`major=1, minor=0, patch=0`）。

## 3. 响应

```text
BridgeResult::GetBundleBuildId(BundleBuildIdInfo {
    build_id: u32,   // 交付构建号
})
```

| 字段 | 语义 |
|---|---|
| `build_id` | 交付构建号：动态端 = `.tela` bundle 的构建序号；静态路径（iOS）= 构建注入的 tela 交付构建序号（**不读 `CFBundleVersion`**）。与 `getAppBuildId` 独立（同一应用可打进不同交付包） |

### 语义边界

- **与应用构建独立**：同一应用版本可对应多个交付包；应用侧构建用 [getAppBuildId](003-getAppBuildId.md)。
- **与宿主壳解耦**：bundle 是 tela 自己的交付物；App 壳构建号（`CFBundleVersion`）不在桥范围内，本桥不读、不依赖。
- 交付语义版本用 [getBundleVersion](004-getBundleVersion.md)。

## 4. Host 实现

本桥为本地能力，**应同一轮回投**。来源：**五端统一 = tela 构建期注入的交付构建序号**（动态端 = `.tela` bundle 构建序号；静态路径同样注入，不读 CFBundle 值）。不允许运行期拼装。

## 5. 版本历史

| 版本 | 变更 |
|---|---|
| 1.0.0 | 首版：`build_id` |

## 6. 验收

- 动态路径：`build_id` 与 `.tela` bundle 构建序号一致。
- 静态路径（iOS）：`build_id` = 构建注入的交付构建序号，与 `CFBundleVersion` 解耦、与应用构建号独立。
- 与应用维度隔离：修改交付注入常量不影响 `getAppBuildId` 结果。
- 同轮回投断言。
