# device/004-getBundleVersion

> **状态：📐 设计已确认，MVP 定稿。** `std.device.getBundleVersion`：交付构建语义版本拉取。体系规则见 [000-宿主桥总览](../000-宿主桥总览.md)，通用类型见 [通用模型](../通用模型/README.md)。

## 1. 结论

`std.device.getBundleVersion` 让页面查询**交付构建的语义版本**（`Version { major, minor, patch }`）。用于交付渠道判断、更新引导（对比 bundle 语义版本）。

**本桥只回答"当前交付包是什么版本"**；交付构建序号与应用侧四桥是独立原子桥（[getAppName](001-getAppName.md) / [getAppVersion](002-getAppVersion.md) / [getAppBuildId](003-getAppBuildId.md) / [getBundleBuildId](005-getBundleBuildId.md)），按需组合。

## 2. 请求

```text
BridgeRequest::GetBundleVersion { version: VersionPolicy }
```

首期版本：`std.device.getBundleVersion` v1（`major=1, minor=0, patch=0`）。

## 3. 响应

```text
BridgeResult::GetBundleVersion(BundleVersionInfo {
    version: Version,   // 交付构建语义版本 {major, minor, patch}
})
```

| 字段 | 语义 |
|---|---|
| `version` | 交付构建语义版本，通用三元组（[通用模型](../通用模型/README.md) §2），直接比较 |

### 语义边界

- **动态路径**（webview/win32/macOS/android）：交付语义版本来自 `.tela` bundle 清单的语义版本。
- **静态路径**（iOS）：交付语义版本 = **构建注入的 tela 交付版本常量**（不读 `CFBundleShortVersionString`）——tela 应用链进 App，但交付版本由 tela 构建管线注入，**不与 App 壳绑定**。
- **交付维度与宿主壳解耦**：bundle 是 tela 自己的交付物；App 壳版本（CFBundle 值）不在桥范围内，本桥不读、不依赖。
- **与应用维度强制隔离**：交付语义版本（本桥）与应用语义版本（[getAppVersion](002-getAppVersion.md)）独立注入、独立演进；数值相同不代表语义耦合。
- 交付语义版本与交付构建序号独立演进（同一语义版本可对应多个构建序号）。

## 4. Host 实现

本桥为本地能力，**应同一轮回投**。来源：**五端统一 = tela 构建期注入的交付版本常量**（动态端 = `.tela` bundle 清单语义版本；静态路径同样注入，不读 CFBundle 值）。不允许运行期拼装。

## 5. 版本历史

| 版本 | 变更 |
|---|---|
| 1.0.0 | 首版：`version` |

## 6. 验收

- 动态路径：`version` 与 `.tela` bundle 清单实际值一致（三元组直接比较）。
- 静态路径（iOS）：`version` = 构建注入的交付常量，与 `CFBundleShortVersionString` 解耦。
- 与应用维度隔离：修改交付注入常量不影响 `getAppVersion` 结果。
- 同轮回投断言。
