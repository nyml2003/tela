# 预留：命名空间与机制（MVP 不注册）

> **状态：📐 命名空间已定，能力不注册。** 本文记录 MVP **只占命名空间、不注册能力**的预留项与后置机制。canIUse 对下列所有能力回 `UnknownCapability`（[000-宿主桥总览](000-宿主桥总览.md) §2.4）。注册即承诺全端实现：任何能力进入 std 枚举时，五个 Target 必须同时具备实现，否则不注册。

## 1. 预留能力

| 命名空间 | 计划能力 | 语义方向 | 进 std 前提 |
|---|---|---|---|
| `std.pubsub` | `subscribe` / `unsubscribe` / `publish` | 消息总线：topic 纯字符串、精确匹配、纯实时，`BridgeEvent::Message` 推送 | 动作桥需求出现（MVP 只做只读桥） |
| `std.route` | `open` / `canOpen` / `onChange` | tela 自定义路由协议（URI 规范见 §2） | 路由协议定稿 + 全端实现就绪 |
| `std.storage` | `getValue` / `setValue` / `removeValue` / `listKeys` | 持久化存储（KV，值字节或 JSON） | 出现真实消费方 |
| `std.user` | `profile`（昵称/头像/标识）等 | 依赖宿主身份系统 | 出现真实消费方并统一语义 |
| `std.device` 扩展 | `clipboard`（读写剪贴板） | 系统剪贴板 | webview HTTPS 约束与权限语义收敛 |
| `std.position` 扩展 | `watch`（连续定位） | 订阅式推送（约定 topic `std.position.update`） | pubsub 消息推送成熟后接入 |
| `std.config` 扩展 | 配置变更订阅 | 推送式 | 出现真实消费方 |

## 2. 路由 URI 规范草案（`std.route`）

目标格式符合 **RFC 3986 URI**：

```text
tela://<page_id>/<path>?<query>      // 内部路由：切换/导航到页面
https://<host>/<path>                // 外部深链：交给系统默认处理
tela-app://...                       // 其他 tela 应用间路由（未来）
```

- `<page_id>` 与页面唯一标识一致（`[a-z0-9_]`、单段）；`<path>`/`<query>` 语义由应用定义。
- 计划能力：
  - `open(uri)`：请求宿主打开 URI（内部导航或外部深链）。
  - `canOpen(uri)`：查询宿主能否处理该 URI。
  - `onChange`：路由变化订阅（经 pubsub 推送）。
- 本草案是**设计占位**，实现前需单独成文。

## 3. 后置机制（回归项）

MVP 只做只读桥，以下机制后置，回归时按 breaking change 处理（[000-宿主桥总览](000-宿主桥总览.md) §10.2）：

| 机制 | MVP 现状 | 回归设计方向 |
|---|---|---|
| 消息总线三桥 | 无（MVP 只读桥） | `subscribe` / `unsubscribe` / `publish`，topic 纯字符串精确匹配，`BridgeEvent::Message` 推送 |
| `Topic` 类型 | 不注册 | `Topic(String)`：非空、UTF-8、≤256 字节，codec 层校验 |
| 结构化 `Namespace`（段数组） | 无 | 段数组 + 解析/校验/显示，wire 序列化段数组零解析 |
| 页面命名空间 | 无 | `page.<page_id>.<root_tag>`，root_tag 由 host 签发（防伪造），页面间通信 |
| 命名空间注册 | 无 | `registerNamespace` 先注册后使用、前缀重叠拒绝（`NamespaceConflict`） |
| 级联订阅 | 无 | 向命名空间发布 → 子命名空间订阅者收到（前缀匹配 + 按订阅者去重） |
| `std` 前缀发布保护 | 无 | `std` 保留命名空间 guest 只读订阅、不可 publish（防伪造宿主消息） |
| 消息缓存/回放 | 无 | retained/消费次数等发布策略，出现真实需求再设计 |
| 订阅 payload 上限 | 无 | 64 MiB（对齐 ABI），按需收紧 |
| 配置变更订阅 | 纯拉取 | 推送式（订阅 `std.config.*`） |

## 4. 预留不注册的验收

- canIUse 对 §1 全部预留能力回 `UnknownCapability`（含 `std.pubsub.subscribe`、`std.route.open`、`std.storage.getValue`、`std.user.profile`、`std.device.clipboard`、`std.position.watch`）。
- `ListCapabilities` 不含任何预留能力。
- 各 Target 静态表不含预留能力（注册即实现）。
