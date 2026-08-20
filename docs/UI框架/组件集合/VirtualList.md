# VirtualList

> 虚拟化列表：只布局可见窗口内的项（`overscan` 缓冲），适合长列表。体必须是 `{|item| ...}` 闭包形式。

## Props

| 属性 | 类型 | 说明 |
|---|---|---|
| `items` | `Vec<Item>` | 可见项数据源（虚拟窗口内） |
| `total_items` | `usize` | **必填**：总项数 |
| `key` | `impl Into<String>` | **必填**：项 key 基础 |
| `item_height` | `f32` | 项高度（定高） |
| `item_spacing` | `f32` | 项间距 |
| `overscan` | `usize` | 可见窗口外缓冲项数 |
| `first_item_index` | `usize` | 起始项索引（滚动恢复） |

## 示例

```rust
ui!(build {
    <VirtualList
        key={"logs"}
        items={visible_logs}
        total_items={logs.len()}
        item_height={24.0}
        item_spacing={2.0}
        overscan={8}
    >
        {|log| ... /* 每项返回 ViewOutput */ }
    </VirtualList>
})
```
