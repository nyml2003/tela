# Stack

> 层叠容器：子节点共享同一布局框叠放（后绘制的在上）。公共属性见 [README](README.md)。

## Props

仅公共属性。

## 示例

```rust
ui!(build {
    <Stack key={"badge"} width={Size::fixed(48.0)} height={Size::fixed(48.0)}>
        <Frame fill={Fill::Solid(Color::WHITE)}>
            <Text value={"底"} />
        </Frame>
        <Frame width={Size::fixed(16.0)} height={Size::fixed(16.0)} fill={Fill::Solid(Color::RED)} />
    </Stack>
})
```

常用于徽标叠加、选中遮罩等。
