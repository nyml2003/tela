# device/001-getAppName

> **状态：📐 设计已确认，MVP 定稿。** `std.device.getAppName`：应用显示名拉取。体系规则见 [000-宿主桥总览](../000-宿主桥总览.md)，通用类型见 [通用模型](../通用模型/README.md)。

## 1. 结论

`std.device.getAppName` 让页面查询**应用显示名**（如 "文件管理器"）。用于"关于"页、标题栏展示、诊断信息。

**本桥只回答"应用叫什么"**；应用语义版本/构建序号、交付版本/构建序号是独立原子桥（[getAppVersion](002-getAppVersion.md) / [getAppBuildId](003-getAppBuildId.md) / [getBundleVersion](004-getBundleVersion.md) / [getBundleBuildId](005-getBundleBuildId.md)），按需组合。

## 2. 请求

```text
BridgeRequest::GetAppName { version: VersionPolicy }
```

首期版本：`std.device.getAppName` v1（`major=1, minor=0, patch=0`）。

## 3. 响应

```text
BridgeResult::GetAppName(AppNameInfo {
    name: String,   // 应用显示名
})
```

| 字段 | 语义 |
|---|---|
| `name` | 应用显示名（构建期注入，非运行期拼装） |

## 4. Host 实现

本桥为本地能力，**应同一轮回投**。来源：**五端统一 = tela 构建期注入的应用显示名常量**（iOS 静态路径同样注入，不读 `CFBundleDisplayName`/`CFBundleName`——显示名是 tela 应用自己的属性，不与 App 壳绑定）。

## 5. 版本历史

| 版本 | 变更 |
|---|---|
| 1.0.0 | 首版：`name` |

## 6. 验收

- 各端返回构建期注入的显示名，无缺省假值（iOS 静态路径与 App 壳显示名解耦）。
- 同轮回投断言。
