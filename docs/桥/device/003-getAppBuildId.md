# device/003-getAppBuildId

> **状态：📐 设计已确认，MVP 定稿。** `std.device.getAppBuildId`：应用构建序号拉取。体系规则见 [000-宿主桥总览](../000-宿主桥总览.md)，通用类型见 [通用模型](../通用模型/README.md)。

## 1. 结论

`std.device.getAppBuildId` 让页面查询**应用构建序号**（每次构建递增的数字）。用于诊断、构建溯源、增量更新判断。

**本桥只回答"应用是第几次构建"**；应用显示名/语义版本、交付侧版本/构建序号是独立原子桥（[getAppName](001-getAppName.md) / [getAppVersion](002-getAppVersion.md) / [getBundleVersion](004-getBundleVersion.md) / [getBundleBuildId](005-getBundleBuildId.md)），按需组合。

## 2. 请求

```text
BridgeRequest::GetAppBuildId { version: VersionPolicy }
```

首期版本：`std.device.getAppBuildId` v1（`major=1, minor=0, patch=0`）。

## 3. 响应

```text
BridgeResult::GetAppBuildId(AppBuildIdInfo {
    build_id: u32,   // 应用构建序号
})
```

| 字段 | 语义 |
|---|---|
| `build_id` | 应用构建序号（构建期常量，每次构建递增） |

### 语义边界

- **与交付构建独立**：`build_id` 是应用自身构建序号；同一应用可打进不同交付包，交付侧用 [getBundleBuildId](005-getBundleBuildId.md)。
- **App 壳构建不在桥范围**：宿主壳（如 iOS `CFBundleVersion`）是宿主的事，本桥不读、不依赖。

## 4. Host 实现

本桥为本地能力，**应同一轮回投**。来源：**五端统一 = tela 构建期注入的应用构建序号常量**（iOS 静态路径同样注入，不读 `CFBundleVersion`）。不允许运行期拼装。

## 5. 版本历史

| 版本 | 变更 |
|---|---|
| 1.0.0 | 首版：`build_id` |

## 6. 验收

- 各端返回与构建注入实际值一致的构建序号。
- 与交付维度隔离：修改应用注入常量不影响 `getBundleBuildId` 结果。
- 同轮回投断言。
