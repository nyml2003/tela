# UI 框架（ui! DSL）

> **状态：📖 使用参考。** Tela 声明式 UI DSL：`ui!` 宏构建 UI 树，Kernel 布局/渲染，宿主壳驱动输入。全部组件与 Props 见 [组件集合](组件集合/README.md)。

## 快速上手

```rust
use tela_ui_dsl::{ViewBuild, ViewOutput, ViewResult, ui};

ui!(build {
    <View key={"root"} width={Size::fixed(320.0)} height={Size::fixed(200.0)}>
        <Text value={"Hello Tela"} font_size={16.0} />
    </View>
})
```

- 返回值：`ViewResult<ViewOutput<A>>`（`A` = 应用动作枚举）；子函数输出用 `{ ... }` 内联组合
- 组件：`Column`/`Row`/`Frame`/`View`/`Stack`/`ScrollView`/`Text`/`Icon`/`Image`/`ActionTarget`/`For`/`VirtualList`/`Fragment`

## 文档索引

| 文档 | 内容 |
|---|---|
| [组件集合](组件集合/README.md) | 全部组件 + 公共属性速查表（组件各一篇） |
| （待写）动作与信号 | `ActionTarget`/`FrameCoordinator`/`Signal`/`@watch` |
| （待写）文本输入 | `TextInputSpec`/`bind_id`/`ValueChange`/多行编辑 |
| （待写）应用集成 | App 装配 + `ensure_frame` 流水线 + 页面路由 |
| （待写）样式与布局 | 主题色板 / `LayoutConcern` / `Insets` / `Size` / `Fill` |

参考实现：`apps/win32-editor/`（三页 + 导航 + 多行编辑器，桌面静态壳）、`apps/mobile-demo/`（列表 + 搜索 + 虚拟化）。

## 开发环境：Zed 中宏内跳转

`ui!` 是 proc macro；Zed（rust-analyzer）对它支持情况：

- **高亮正常但点击跳转失效**：宏展开成功但语义 span 映射缺失。按序检查：

1. **确认 proc macro 展开开启**（Zed 设置）：

```jsonc
{
  "lsp": {
    "rust-analyzer": {
      "initialization_options": {
        "procMacro": { "enable": true }
      }
    }
  }
}
```

2. **确认宏 crate 可编译**（rust-analyzer 执行展开需要构建 `tela-ui-dsl-macros` 的 dylib）：

```bash
cargo check -p tela-ui-dsl-macros
```

3. **跳转预期**：`ui!` 块内 `F12`/`Cmd+点击` 组件标签会跳到 `crates/ui/dsl-macros/src/lib.rs` 的对应生成函数（如 `generate_text`）——这是 rust-analyzer 对宏内符号的常规映射；完全无反应才需要排查。

4. **Zed 语义索引**（0.17x+，覆盖宏展开符号的项目级导航）：

```jsonc
{
  "semantic_index": { "enabled": true }
}
```

- 仍失效：升级 Zed；`zed: open log` 搜索 `rust-analyzer` 的 proc macro server 报错；备选工作流用 hover 看展开预览、对生成函数名全局搜索。
