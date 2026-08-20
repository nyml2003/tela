# device/002-getAppVersion

> **状态：📐 设计已确认，MVP 定稿。** `std.device.getAppVersion`：应用语义版本拉取。体系规则见 [000-宿主桥总览](../000-宿主桥总览.md)，通用类型见 [通用模型](../通用模型/README.md)。

## 1. 结论

`std.device.getAppVersion` 让页面查询**应用语义版本**（`Version { major, minor, patch }`）。用于更新引导、版本兼容判断（配合 canIUse 之外的策略逻辑）、版本展示。

**本桥只回答"应用语义版本是多少"**；应用显示名/构建序号、交付版本/构建序号是独立原子桥（[getAppName](001-getAppName.md) / [getAppBuildId](003-getAppBuildId.md) / [getBundleVersion](004-getBundleVersion.md) / [getBundleBuildId](005-getBundleBuildId.md)），按需组合。

## 2. 请求

```text
BridgeRequest::GetAppVersion { version: VersionPolicy }
```

首期版本：`std.device.getAppVersion` v1（`major=1, minor=0, patch=0`）。

## 3. 响应

```text
BridgeResult::GetAppVersion(AppVersionInfo {
    version: Version,   // 应用语义版本 {major, minor, patch}
})
```

| 字段 | 语义 |
|---|---|
| `version` | 应用语义版本，通用三元组（[通用模型](../通用模型/README.md) §2），直接比较，不解析外部字符串 |

### 语义边界

- **应用维度与交付维度强制隔离**：应用语义版本（本桥）与交付语义版本（[getBundleVersion](004-getBundleVersion.md)）**独立注入、独立演进**——同一应用版本可对应多个交付包，反之亦然；两维度数值相同不代表语义耦合。
- **App 壳版本不在桥范围**：宿主壳（如 iOS `CFBundleShortVersionString`）是宿主的事，本桥不读、不依赖；应用维度一律来自 tela 自身构建注入。
- 预发布/渠道后缀（如 `-beta.1`）不在三元组内表达；需要时由未来版本扩展。
- 宿主能力版本经 `std.base.canIUse` 查询，本桥不重复提供。

## 4. Host 实现

本桥为本地能力，**应同一轮回投**。来源：**五端统一 = tela 构建期注入的应用语义版本常量**（iOS 静态路径同样注入，不读 `CFBundleShortVersionString`）。不允许运行期拼装或解析外部字符串。

## 5. 版本历史

| 版本 | 变更 |
|---|---|
| 1.0.0 | 首版：`version` |

## 6. 验收

- 各端返回与构建注入实际值一致的语义版本（三元组直接比较）。
- 与交付维度隔离：修改应用注入常量不影响 `getBundleVersion` 结果。
- 同轮回投断言。
