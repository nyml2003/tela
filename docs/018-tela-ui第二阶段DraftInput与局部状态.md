# 018-tela-ui 第二阶段：DraftInput 与局部状态闭环

> 历史实施记录：本文的独立 `LocalStateRuntime`、`InstancePath` 和 `UiIntent::Commit` 已被
> `tela-ui-dsl` 的组件 owner、`ComponentIdentity`、typed Output 与 `FrameCoordinator`
> 事务替代。新开发必须遵守
> [036-事件系统与组件生命周期机制](036-事件系统与组件生命周期机制梳理.md)，不得复刻本文运行时。

> **状态：✅ 已完成。**  
> **前置：** [017-tela-ui第一阶段实施目标.md](017-tela-ui第一阶段实施目标.md) 已完成；[015-tela-ui与局部交互状态.md](015-tela-ui与局部交互状态.md) 是本阶段的架构边界。

## 1. 目标

在 `tela-ui` 实现第一个完整的局部交互状态闭环：`DraftInput` 在组件实例内维护编辑草稿，处理 IME 输入、确认、取消、外部值同步与组件卸载；仅在具有业务含义的确认点向外派发 `UiIntent::Commit`。

本阶段以 `tela-demo` 的文件操作弹窗为端到端落点。用户打开新建、重命名或标签操作后，输入过程不得直接写入业务 Session 或 Model；确认操作仍沿用现有 Controller 和 Model 写入路径。

## 2. 已确认的设计决策

| 主题 | 决策 |
| --- | --- |
| 草稿所有权 | 草稿、IME 组合态、脏标记和冲突标记属于 `DraftInput` 实例；外部业务值仍归上层容器所有。 |
| 实例标识 | 由 `tela-ui` 运行时根据组件树自动生成不可见的 `InstancePath`；页面和普通组件调用者不管理 Tela key，也不暴露为路由或语义标识。 |
| 外部值冲突 | 草稿已脏时收到不同外部值，保留本地草稿并记录 `conflicted`；不得静默覆盖用户正在编辑的内容。 |
| 确认时机 | `Blur` 和非 IME 组合期间的 `Enter` 都执行一次 `commit_if_dirty`；`compositionend` 只结束组合，不提交。 |
| 取消 | `Escape` 或弹窗取消放弃草稿，恢复最近一次外部值，不写业务 Store。 |
| 生命周期 | 父组件不再出现在渲染树中即释放其局部状态；操作弹窗关闭后再次打开必须从最新外部值重新初始化。 |
| 上行协议 | 复用 `UiIntent::Commit` 和 `IntentTarget`；不为 DraftInput 扩展 `tela-contract`。 |
| 默认输出 | 普通输入过程不派发 `Preview`；实时搜索等需求以后由独立、显式的组件或配置承担。 |

## 3. 实施范围

### 3.1 `tela-ui` 的局部状态运行时

在 `crates/tela-ui` 增加仅服务组件实例的局部状态运行时。它至少要提供以下能力：

- 根据渲染上下文分配和追踪 `InstancePath`，不要求业务代码提供稳定 key。
- 保存 `DraftInputState`：最近外部值、当前草稿、是否脏、是否处于 IME 组合、是否存在外部值冲突，以及本轮草稿的提交版本。
- 每轮渲染同步外部值：干净实例立即跟随外部值；脏实例遇到不同外部值时仅更新“最近外部值”并置冲突标记。
- 依据本轮已见到的组件树回收未见实例；回收后不保留草稿、焦点或冲突状态。

该运行时只解决局部状态的寻址与生命周期。本阶段继续使用 Demo 现有的显式脏组件刷新机制，不引入 Signal 自动依赖追踪或通用 Scheduler。

### 3.2 `DraftInput` 分子组件

在 `tela-ui` 提供主题中立的 `DraftInput`，将 `Input` 原子控件与草稿状态协议组合起来。组件需要接收受控外部值、`IntentTarget`、占位文本、焦点与禁用等展示配置，并对宿主事件处理以下语义：

| 事件 | 局部状态与输出 |
| --- | --- |
| `input(value)` | 更新草稿和脏标记；不派发业务 Intent。 |
| `compositionstart` | 进入组合态。 |
| `compositionend` | 退出组合态；不提交。 |
| `blur` | 仅在草稿相对最近外部值有变更时派发一次 `UiIntent::Commit`。 |
| `Enter` | 非组合态下执行与 `blur` 相同的 `commit_if_dirty`；随后发生的 `blur` 不得重复提交。 |
| `Escape` / `cancel` | 丢弃草稿，恢复最近外部值，清除脏和冲突标记；不派发 Intent。 |

`DraftInput` 应暴露只读快照供调用方观察当前草稿和 `conflicted`，但不把冲突直接解释为领域错误或弹出业务提示。视觉主题可以基于该快照增加提示样式。

提交后的草稿在下一次外部值同步前仍作为本地基准保留；当上层回传相同值时，组件转为干净状态。若上层回传不同值，则按冲突规则保留本地内容。

### 3.3 浏览器 Canvas 输入桥

`web/src/main.ts` 的隐藏 `textarea` 从只转发 `input` 扩展为转发：

- `compositionstart`、`compositionend`；
- `input`；
- `keydown` 中的 `Enter` 和 `Escape`；
- `blur`。

浏览器端在 IME 组合期间不得将 `Enter` 误当作确认，并应阻止确认键向隐藏 `textarea` 写入换行。Wasm/Demo 输入入口把这些事件送至当前聚焦的 `DraftInput` 实例，而非直接写入 `session.operation.value` 或 `session.query`。

这只是宿主端口的补齐，不改变 `tela-contract` 的事件模型或新增跨端协议。

### 3.4 `tela-demo` 文件操作弹窗集成

将 `crates/tela-demo/src/presentation/operation.rs` 中直接绑定 `operation.value` 的 `tela_widgets::Input` 替换为 `tela_ui::DraftInput`。

- `DraftInput` 的提交 Intent 由现有 Controller 写回 `operation.value`。
- 用户点弹窗的确认按钮后，既有命令路径才将操作写入 Model；取消、关闭和重新打开不留下旧草稿。
- 新建、重命名、标签三种操作都使用同一套草稿语义。
- 外部更新与本地编辑冲突时，Demo 可观察到 `conflicted`，但本阶段不增加领域冲突对话框或修改文件领域数据。

## 4. 明确不在本阶段处理的内容

- Signal 自动依赖追踪、`computed`、`effect` 与全局批量调度器。
- `Dialog`、`Popover`、`Menu`、`SplitPane`、`ListItem` 等其他分子组件。
- 实时输入 `Preview`、搜索防抖或网络副作用编排。
- 领域级冲突解决策略、持久化草稿恢复或跨窗口共享局部状态。
- 让业务页面显式传入或维护组件 key。

## 5. 验收标准

### 5.1 单元与组件测试

- 首次挂载从外部值初始化；干净状态的外部更新立即同步。
- 输入只更新局部草稿；`blur` 和 `Enter` 对同一编辑最多形成一次提交。
- IME 组合期间不提交，`compositionend` 后仍需 `blur` 或 `Enter` 才提交。
- 取消恢复最近外部值且不产生 `UiIntent::Commit`。
- 脏草稿遇到外部值更新时保留内容并标记冲突；卸载后重新挂载不恢复旧草稿。

### 5.2 宿主与 Demo 验收

- 浏览器桥能将 `input`、组合开始/结束、`Enter`、`Escape` 和失焦送到当前 `DraftInput`。
- 文件操作弹窗的新建、重命名、标签操作可完成确认、取消、关闭再打开；旧草稿不会泄漏到下一次打开。
- 现有文件树切换、表格、工具栏以及非操作弹窗路径保持可用。

### 5.3 工程门禁

完成实现后执行：

```bash
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
node ops/src/interface/cli.ts check
node ops/src/interface/cli.ts build
node ops/src/interface/cli.ts verify bundle
cargo fmt --all -- --check
git diff --check
```

## 6. 完成定义

当 `DraftInput` 的草稿状态已经由 `tela-ui` 自动寻址和回收，浏览器 IME/确认事件完整进入该组件，文件操作弹窗不再在输入时直接写业务状态，并且上述测试与门禁均通过时，本阶段完成。

## 7. 完成记录

- `crates/tela-ui/src/local_state.rs` 提供自动 `InstancePath`、草稿同步、冲突标记、提交去重和
  未见/显式父卸载回收；`crates/tela-ui/src/draft_input.rs` 将其投影为主题中立的 `DraftInput`。
- `tela-demo` 的顶部搜索和文件操作弹窗均从该运行时读取草稿；输入过程不再直接改
  `session.query` 或 `session.operation.value`，仅 `UiIntent::Commit` 经 Controller 写回。
- 浏览器隐藏 `textarea` 现转发 `input`、IME composition、`Enter`、`Escape` 与 `blur`；当前由
  `tela-webview-sdk` WebView 壳接入同一组应用运行时方法。
- 本阶段原始验收曾使用已删除的直载 demo 路径；当前工程门禁以 `ops check`、`ops build` 与
  `ops verify bundle` 为准，定向测试覆盖草稿初始化、IME、取消、冲突、卸载、新建/重命名/标签操作。
