# Column

> 垂直容器：子节点沿主轴（纵向）依次排列。公共属性见 [README](README.md)。

## Props

仅公共属性（`gap` 控制子间距，`cross_align` 控制水平对齐）。

## 示例

```rust
ui!(build {
    <Column
        key={"settings"}
        width={Size::fixed(viewport.width)}
        height={Size::fixed(viewport.height - TOP_BAR_H)}
        padding={Insets { top: 24.0, right: 16.0, bottom: 0.0, left: 16.0 }}
        gap={16.0}
    >
        <Text value={"设置"} font_size={20.0} />
        <Text value={"字体大小"} font_size={14.0} />
    </Column>
})
```

参考：`apps/win32-editor/src/presentation.rs`。
