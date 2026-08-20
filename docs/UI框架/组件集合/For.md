# For

> 静态列表循环：对 `each` 迭代源逐项生成子节点。`key` 必填（生成项的基础 SemanticKey，宏会追加索引段）。体必须是 `{|item| ...}` 闭包形式。

## Props

| 属性 | 类型 | 说明 |
|---|---|---|
| `each` | `Vec<Item>`（可迭代） | **必填**：迭代源 |
| `key` | `impl Into<String>` | **必填**：项 key 基础（如 `"@for-"`） |

## 示例

```rust
ui!(build {
    <For each={entries} key={"browse.item"}>
        {|entry| ... /* 每项返回 ViewOutput */ }
    </For>
})
```

项 key 形如 `browse.item@for-0`，Kernel 用它在重建间保持项身份（焦点/滚动/状态稳定）。**不要**在项内再写重复的静态 key。

参考：`apps/mobile-demo/src/presentation.rs`（文件列表）、`crates/ui/dsl/tests/ui_macro.rs`。
