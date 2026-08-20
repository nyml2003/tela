# Frame

> 单子节点尺寸边界容器：承载交互（点击/输入/焦点）与视觉（填充/边框/阴影）槽位，是按钮、输入框等可交互元素的基本包裹。公共属性见 [README](README.md)。

## Props

仅公共属性。`clickable` + `input` + `bind_id` 的组合定义可交互元素：

```rust
// 按钮
<Frame key={"btn"} width={Size::fixed(72.0)} height={Size::fixed(30.0)}
       fill={Fill::Solid(ACCENT_SOFT)} clickable={true}>
    <Text value={"设置"} />
</Frame>

// 文本输入框（多行）
<Frame key={"editor"} width={Size::fixed(w)} height={Size::fixed(h)}
       input={TextInputSpec::new(TextInputKind::Multiline)}
       bind_id={"win32.editor.input"}
       clickable={true}>
    <Text value={document} />
</Frame>
```

注意：`<Frame>` **必须有一个真实子节点**（空 Frame 编译报错）。

参考：`apps/win32-editor/src/presentation.rs`（按钮、编辑器输入框）。
