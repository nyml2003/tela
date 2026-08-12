# 015-tela-ui 与局部交互状态

> **状态：🔧 设计已确认，尚未实现。** 本文定义 `tela-ui` 作为 `tela-widgets` 之上的
> 主题无关分子组件层，以及其局部交互状态、Signal 精准更新和命令调度边界。

## 1. 分层

```
业务容器 / Store（业务 Signal、命令、异步副作用）
                         ▲ UiIntent
tela-ui（分子组件、组件实例状态、Scheduler、UiIntent 路由）
                         ▲ UiAction
tela-widgets（原子控件、BindId、受控值、焦点、IME 宿主适配）
                         ▲
tela-core（值树、布局、身份、命中、焦点、ViewStateStore）
```

| 层 | 负责 | 不负责 |
|---|---|---|
| `tela-core` | 原语、值语义 `UiNode`、布局、命中、焦点、滚动、modal 栈和跨帧视图状态 | Signal、业务数据、组件实例状态、调度、异步副作用 |
| `tela-widgets` | Button/Input/Checkbox 等原子控件；受控值、`BindId`、IME 与基础焦点语义 | 组合工作流、业务命令、跨组件局部状态 |
| `tela-ui` | Form/Table/Select/Cascader 等分子组件；Dialog/Popover/Menu/Tabs/Toolbar；实例状态、批处理和 `UiIntent` | 主题、领域模型、网络/存储、renderer、tela key 管理 |
| 宿主 / Store | 业务 Signal、校验、命令、异步 port 与副作用 | tela-core 内部状态、组件私有交互实现 |

`tela-ui` 主题无关：组件提供结构、slot、状态与交互契约；颜色、字体、间距和图标通过主题或
宿主传入。它不提供文件管理器等领域组件。

## 2. 状态归属

“数据归拢于上，交互自治于下”，但自治状态必须有可验证的运行时所有者，不能放在一次性
`render()` 的 Rust 栈变量中。

| 状态 | 所有者 | 例子 | 向上同步 |
|---|---|---|---|
| Core view state | `tela-core::ViewStateStore` | hover、focus、scroll、modal 输入拦截 | 否 |
| Component interaction state | `tela-ui` 实例 Store | pressed、popover open、输入草稿、drag scratchpad | 仅在语义边界 |
| Business state | 宿主 Store | 已提交字段、校验、文件实体、请求结果 | 是 |

- 普通组件实例由 runtime 的不可见结构路径自动定位；调用方不传 `SemanticKey`。
- 动态集合只能由 `Table`/虚拟列表等集合组件在内部用业务稳定 ID 维持 item state；该 ID 不泄漏
  为 tela 节点 key。
- 局部状态可放入显式 `CacheScope`；它是 `tela-ui` runtime 的缓存域名，不参与 core identity。
  缓存域的释放策略尚未确定，见第 6 节。

## 3. Signal 与刷新

`Signal` 是应用与 `tela-ui` runtime 的状态原语，不进入 `tela-core`：

1. 组件渲染期间读取 Signal，runtime 自动登记 `Signal -> 组件实例` 依赖。
2. Signal 写入只标脏读取它的实例；同一批次重复写入合并为一次重投影。
3. runtime 以组件边界产出新的 `UiNode` 子树；`tela-core::resolve_dirty` 继续负责布局盒复用。
4. `Computed` 只做纯派生，不读取时钟、不执行防抖、节流、网络或其它副作用。

这不取代 core 的 `ViewStateStore`。hover/focus/scroll/modal 仍由 core 管理；`tela-ui` 只保存
业务不可见但组件需要跨帧维持的交互状态。

## 4. 上行意图与调度

`tela-ui` 不直接调用业务 Store action，也不在 `tela-contract` 新增业务 Action 类型。它消费 core
的 `UiAction`，向宿主输出结构化 `UiIntent`：

```rust
enum UiIntent {
    Preview { target: IntentTarget, value: Value },
    Commit { target: IntentTarget, value: Value },
    Invoke { target: IntentTarget },
}
```

- `IntentTarget` 是宿主路由标识，不是 tela `SemanticKey`，也不是节点 `NodeId`。
- `Preview` 仅用于显式开启的高频场景，例如搜索；默认关闭。
- `Commit` 代表业务边界，例如 Input 的 blur、Enter 或 Dialog 确认。
- `Invoke` 代表按钮、菜单项、列表项等命令激活。
- 宿主的 Command Dispatcher 把 `UiIntent` 转为业务命令/Action；业务 Action 可以批量写 Signal，
  异步 port 的发起与成功/失败回写都留在宿主层。

按钮按下、hover、ripple、drag 中间位置属于组件即时反馈，不等待业务 Signal。提交中禁用、错误
提示和结果展示则读取宿主 Signal。

## 5. 高频交互与 IME

`tela-ui::Scheduler` 只在组件层处理局部高频状态与 intent 合并，时间由宿主注入：

| 策略 | 适用 | 行为 |
|---|---|---|
| `Immediate` | 普通命令 | 每次 `Invoke` 进入宿主 Dispatcher |
| `DropWhilePending` | 提交、保存、删除 | 同一目标 pending 时拒绝重复提交 |
| `LatestPerFrame` | 拖拽、Slider | 每帧只保留最新局部值与 Preview |
| `Debounce` | 搜索预览 | 空闲窗口后才输出最新 Preview |
| `Coalesce` | resize、滚动摘要 | 同批次只输出最后一个值 |

- 去重策略由命令/组件声明，不能在通用 Button 中硬编码固定时长。
- `Computed` 不能承担节流；Scheduler 承担时间相关批量调度。
- IME composition 期间仅更新 `DraftInput` 草稿，不输出 Preview 或 Commit；`compositionend` 后才允许
  按组件策略合并，blur/Enter 才产生 Commit。

## 6. 首版范围与开放问题

已确认的首版：直接从 `tela-widgets` 移动 `Form`、`Table`、`Select`、`Cascader`；新增
`Dialog`、`Popover`、`Menu`、`Tabs`、`Toolbar`、`EmptyState`、`DraftInput` 与 `Slider/DragValue`。
迁移为直接移动，不保留 `tela-widgets` 的兼容 re-export。

以下问题必须在实现前做决策：

1. `CacheScope` 的释放时机：容器主动清理、父域卸载清理，或基于容量/时间回收。
2. Dialog/Popover/Menu 的 v1 具体范围：是否一次提供三者，以及各自的 Escape、点击外部、焦点
   保存/恢复的默认规则。
3. `UiIntent` 的 Rust 公共 API：`IntentTarget` 的强类型封装、payload 是否复用 `Value`，以及
   一个 intent 是否允许携带附加字段。
4. 自动依赖追踪的实现：扩展现有 `Signal` 以在 render scope 中收集依赖，或在 `tela-ui` runtime
   新建兼容 Signal facade；无论选项如何，`tela-core` 不依赖它。
5. Scheduler 的宿主时钟、帧回调和微任务端口形状，以及离线测试使用的手动虚拟时钟。
6. 输入的外部值在本地草稿尚未提交时如何处理：默认应以外部业务值覆盖草稿，还是检测冲突并保留
   草稿。该规则需在 `DraftInput` API 定义前确定。

## 7. 验收基线

- 未读取某 Signal 的组件不会被标脏；同批次多次写入只重投影一次。
- Input 在 composition 中不提交；blur/Enter 才产生一个 Commit；显式 Preview 只按 Scheduler 策略输出。
- Dialog/Popover/Menu 的 local state 不泄漏到业务 Store；关闭或缓存域释放后按外部快照重新初始化。
- Slider/drag 在一帧内多次更新只向宿主输出最后一个 Preview，最终释放产生一个 Commit。
- 组件不要求调用方提供 tela key；集合组件的内部稳定 ID 映射在排序、过滤和虚拟窗口移动后保持。
- core 的 `resolve_dirty` 与 Full 输出保持结构等价；`tela-ui` 不向 core、renderer 或宿主平台反向依赖。
