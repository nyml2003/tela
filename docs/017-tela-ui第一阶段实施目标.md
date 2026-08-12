# 017-tela-ui 第一阶段实施目标

> **状态：📋 下一 Goal，尚未开始。** 本文是 [015-tela-ui 与局部交互状态](015-tela-ui与局部交互状态.md)
> 的第一张实施卡：先验证 crate 边界、组合组件迁移和统一意图出口，再处理自动依赖追踪、
> Scheduler 与复杂局部状态。

## 1. 目标

建立 `tela-ui` crate，完成第一批分子组件迁移，并以主题无关的 `Toolbar` 验证“组件输出
`UiIntent`、业务容器处理业务命令”的边界。

本期完成后，`tela-widgets` 只保留原子控件与基础输入/绑定能力；`tela-ui` 成为组合模式、
结构和交互契约的唯一归属。文件管理器演示继续由 Controller/Model 管理业务操作，交互语义不变。

## 2. 必须完成的范围

### 2.1 Crate 与依赖

1. 新建 `crates/tela-ui`，加入 workspace。
2. 建立并验证单向依赖：

```text
tela-core -> tela-widgets -> tela-ui -> demo / host
```

3. `tela-core` 不依赖 Signal、组件运行时或业务组件；`tela-ui` 不依赖 renderer、宿主平台、
网络、文件系统或业务 Store。

### 2.2 组合组件迁移

从 `tela-widgets` **直接移动**下列组件、共享类型和相关测试到 `tela-ui`：

- `Form`
- `FormItem`
- `Select`
- `Cascader`
- `Table`
- `Tr`
- `Td`

不保留 `tela-widgets` 的兼容 re-export。所有调用方改为直接依赖 `tela-ui`，避免两个 crate
同时宣称同一组件的所有权。

### 2.3 最小 `UiIntent`

在 `tela-ui` 定义最小、主题和领域无关的上行意图接口：

```rust
enum UiIntent {
    Preview { target: IntentTarget, value: Value },
    Commit { target: IntentTarget, value: Value },
    Invoke { target: IntentTarget },
}
```

- `IntentTarget` 是业务意图路由值，不是 `SemanticKey`、`NodeId` 或组件实例路径。
- 组件只生成 `UiIntent`，不得直接调用业务 Store action。
- 优先复用现有 `BindId`、值类型与 `UiAction`；本期不因方便而扩展 `tela-contract`。只有现有
  契约无法表达上述最小接口时，才以单独设计说明提出变更。
- `Preview` 仅为接口预留；本期不实现定时、防抖或帧合并调度。

### 2.4 `Toolbar` 样板组件

新增主题无关 `Toolbar` 或 `CommandToolbar`，作为分子组件样板：

- 支持普通按钮项、禁用项、溢出项和 `Invoke` 意图。
- 结构、slot、状态和交互契约属于 `tela-ui`；颜色、字体、间距和图标由 props/theme 参数提供。
- hover、pressed、focus 等即时反馈只使用 core view state 或组件局部状态，不等待业务 Store。
- 不得嵌入文件管理器操作名、文件模型或 Controller。

### 2.5 演示接入

- `tela-demo` 使用迁移后的组合组件和 `Toolbar`。
- 业务操作继续通过 Controller/Model；不得让 `tela-ui` 直接修改文件管理器 Session 或 Model。
- 保持现有模块结构：`application`、`domain`、`host`、`presentation`；不得为了接入把页面代码
  重新集中到单个文件。
- 目录切换、文件选择、弹窗提交/取消和工具栏操作必须保持既有行为。

## 3. 明确不在本期

以下内容留给后续 Goal，不能以“顺手实现”为由混入本期：

- Signal 自动依赖追踪与组件边界精确重投影。
- `CacheScope` 生命周期与局部实例状态 runtime。
- Scheduler、宿主时钟、微任务/动画帧批处理、防抖、节流与重复提交策略。
- IME composition 的 DraftInput 提交策略。
- `Dialog`、`Popover`、`Menu`、`Tabs`、`EmptyState`、`Slider`、`DragValue`。
- `tela-contract` 的业务 Action 扩展、网络/存储/异步副作用。

这些能力的边界和开放问题仍以 [015](015-tela-ui与局部交互状态.md) 为准。

## 4. 实施约束

- 普通组件调用方不管理 tela key；节点身份继续由 `tela-core` 的身份策略处理。
- 动态集合所需稳定业务 ID 只能封装在 `Table`、虚拟列表等集合组件内部，不泄漏为调用方必须
  维护的 tela key。
- 不修改既有业务交互语义，不将 UI 局部状态上提为业务 Model。
- 迁移必须带着测试走：不得只复制实现、删除旧文件后依赖 demo 运行碰巧通过。
- 文档陈述必须与本期最终源码一致；未实现项目继续保留为计划，不标记为完成。

## 5. 验收

### 5.1 结构验收

- workspace 包含 `tela-ui`，依赖方向检查通过。
- `tela-widgets` 不再导出 `Form`、`FormItem`、`Select`、`Cascader`、`Table`、`Tr`、`Td`。
- `tela-ui` 定义并测试 `UiIntent`、`IntentTarget` 与 `Toolbar` 的意图映射。
- `tela-demo` 仅作为 `tela-ui` 的使用方，业务命令仍留在 Controller/Model。

### 5.2 行为验收

- 迁移组件的原有测试全部保留或等价替换。
- `Toolbar` 的普通项、禁用项和溢出项具有明确的 `UiIntent::Invoke` 行为。
- 文件管理器的目录切换、文件选择、modal 提交/取消和工具栏操作不回归。
- 组件调用方无需传 tela key；集合稳定 ID 的使用没有扩散到业务页面。

### 5.3 交付门禁

```bash
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
node ops/src/interface/cli.ts check
node ops/src/interface/cli.ts build all --gpu
node ops/src/interface/cli.ts verify demo
git diff --check
```

最终报告必须列出：实际新增/迁移的 crate 与模块、最终依赖方向、验证结果，以及本期刻意未实现的
能力。
