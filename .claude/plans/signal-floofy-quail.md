# Signal-only 核心抽象：思想、特性推导与实施（P1+P2，接受重写级简化）

## Context

用户确认方法论：先保证思想与抽象，再推导特性，最后实现，完全重写可接受。本次把先前讨论定稿的目标架构（零内容比较 + O(变化)）落为 tela-ui-dsl 的核心抽象，并按它实施——包括删除违背抽象的赘物（`#[memo]` 注解、指纹、值比较）。

## 思想（写进 lib.rs 模块文档）

> **一切动态性皆显式的边；一切更新皆按坐标的定点重入；一切复用皆身份匹配。**

三条规则：① 状态 = 图节点（源/派生），依赖 = 构造时声明的边，写入沿边传脏；② 界面 = 身份树，组件 = 可重入求值单元（边 + 坐标 + 求值器）；③ 读路径零内容比较——diff/指纹/值比较不存在，内容相等只在写路径（短路阻尼）。

特性判定准则：新特性必须两问皆过——动态性是否为显式边？运行时比较是否身份级？

### 特性推导表（实现必须与之一致）

| 特性 | 原理规定 |
|---|---|
| `Signal` | 源节点；相等性短路（写路径阻尼）；版本号 O(1) 新鲜度 ✅ 已有 |
| `Computed` | 派生节点；**边即构造参数**；源变立即重算，值等则传播终止 |
| `#[watch]` props | 树位置对图节点的观察边；derive 组件**唯一**输入形态 |
| identity / `key` / For | 同一坐标系统的三种呈现；复用 = 身份匹配 |
| retained（原 memo） | 求值语义本身：**入边无脏 → 不重求值**；derive 组件默认生效，非 opt-in |
| `inject` | 只许 setup 解析的装配期能力（恒定值不构成边）；本期对 derive 编译期禁止 |
| children | 现阶段 = 自动退出 retained（`Children::empty` 运行时判定）；远期 = 槽位边 |
| viewport | 图节点（本期 signal 化）；focus/hover 为遗留帧级例外（P4 收编） |

## 实施

### 1. `Computed` 原语 — 新建 `crates/ui/dsl/src/computed.rs`
- `pub struct Computed<T> { signal: Signal<T>, _keep_alive: Box<dyn Any> }`（源订阅令牌 RAII 保活；Clone = Rc 共享）。
- 构造器 `computed(&a,f)` / `computed2(&a,&b,f)` / `computed3(..)`；`f: Fn(&A,..)->T`，`T: Clone + PartialEq`。
- 传播：源 listener → 重算 → `signal.set(next)`，`set` 相等性短路防虚假传播；单线程顺序无 glitch；文档注明 eager 语义（源写即算，v2 可演进惰性）。
- API：`get()/with()/version()/id()`（id = 内部 signal 的 SignalId）。

### 2. `WatchSignal` 统一入口 — `runtime.rs` + `view.rs`
- sealed trait `WatchSignal { fn id(); fn subscribe_erased(l) }`；`Signal<T>`、`Computed<T>` 实现。
- `ViewBuild::watch_source`（view.rs:1242）改收 `&impl WatchSignal`；宏生成代码不变。

### 3. derive 契约升格为默认（重写级简化）— `dsl-macros/src/derive.rs`
- **删除 `#[memo]` 容器属性与全部指纹机制**（`XMemoFingerprint`、`__tela_memo_fingerprint`、逐字段比较、PartialEq/Clone 断言、`attributes(.. memo)`）。
- derive 组件字段只允许：`#[watch] Signal<T>` / `#[watch] Computed<..>` / `key`。`Plain/Defaulted/Option/Inject/Provide` 一律编译错误，错误信息给出迁移指引（数据→signal、恒定能力→setup inject、常量→写进 view 体）。
- watch 类型检查（derive.rs:411）扩展接受 `Computed<..>`。
- **所有 derive 组件默认生成 retained 分支**：命中判定 = watch 字段 `id()` 相等（u64）+ 缓存子树订阅未脏；children 非空自动走全量（现有 `Children::empty` 判定）；`#[memo]` 概念消失。
- 手写 `DslComponent`（foundation 等）不变，天然是逃生通道（永不 retained）。

### 4. `view.rs` / `memo.rs` / `frame.rs`
- `memo_hit` 收 `impl FnOnce(&dyn Any) -> bool`；`memo_record` 存实例快照（字段更名 `inputs`）。
- 命中/记录/事务三段式、脏检查（含缓存子树全部 watch scope）、retain_subtree、身份收集全部保留。

### 5. viewport 收编为图节点 — `app-runtime` + agent-demo
- `Application` 持 `viewport_signal: Signal<Viewport>`；`set_viewport` 同步 set（相等短路）；`FrameContext` 暴露之；全局失效路径保留（宿主逻辑）。
- agent-demo：`TracePanel` 的 width/height → `#[watch] viewport: Signal<Viewport>` 内部自算；`DraftField` 退出 retained（focused 为宿主态、渲染轻量——保留 derive 但因 focused 普通字段不再合法，改为**去掉 derive 或把 focused 并入父级**：取简单方案——DraftField 还原为普通函数节点，draft 订阅并入 ChatPanel 后续迭代）；`AgentViewProps` 加 `viewport_signal`；新增 `computed` 活样例（`turns = computed(&messages, |m| m.len()/2)`）。

### 6. 测试与基准
- 新建 `tests/computed.rs`：源变→下游脏；**源变但值等→零脏**（虚假传播吸收）；经 Computed 的 watch 订阅 `take_dirty` 正确；computed2 双源。
- `render_memo.rs`：删 `props_change_..` 用例（通道不存在）；组件去 `#[memo]` 注解（默认 retained）；新增嵌套用例（Outer watch a > Inner watch b：a 变 Outer miss/Inner 命中；b 变双 miss）。
- trybuild：`derive_rejects_plain_props.rs`（普通字段编译错误 + 迁移指引文案）替换 `memo_requires_partial_eq_props.rs`。
- `frame_cost.rs`：`WatchedRow` 的 label 并入 row signal；三路对照语义不变（memoized 列 = 默认 retained）。
- cc-remote `DraftField`：derive 含 plain 字段不再合法 → 去掉 derive 改回普通函数（订阅由 draft 所在路径的组件承接或维持全量渲染，取最小改动）。

## 审计结论（写进模块文档的"边界"节）

- **绑定级完全声明**：数据通道达成（derive 结构保证 + computed 边即参数）；例外清单：focus/hover/scroll/modal/时钟（帧级，P4）、手写 impl、全局可变状态。
- **运行时比较**：只剩身份级（SignalId/脏位/版本）+ 写路径短路；**唯一内容级残留 = 布局子树指纹**，P3 UiNode Rc 子树后变 ptr eq。
- 每帧 O(n) 树组装/resolve 为非比较的 O(变化) 缺口，3A 范畴。

## 验证
1. `cargo test --workspace` 全绿（computed/render_memo/trybuild/agent-demo/cc-remote）
2. agent-demo：打字时 TracePanel 零渲染；RunExample 穿透；resize 派生宽度正确；turns 计数随消息更新
3. `frame_cost --nodes 200 --iterations 200`：retained 路线持平或略降
