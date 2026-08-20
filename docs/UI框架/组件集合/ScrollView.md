# ScrollView

> 滚动容器：内容超出时按 `overflow={Overflow::Scroll}` 滚动；滚动状态由 `ViewStateStore` 保持（滚动偏移跨帧保留）。公共属性见 [README](README.md)。

## Props

仅公共属性。必配 `overflow={Overflow::Scroll}` 与 `clip={true}`：

```rust
ui!(build {
    <ScrollView
        key={"editor.scroll"}
        width={Size::fixed(viewport.width)}
        height={Size::fixed(viewport.height - TOP_BAR_H)}
        padding={Insets { top: 16.0, right: 16.0, bottom: 16.0, left: 16.0 }}
        overflow={Overflow::Scroll}
        clip={true}
    >
        <Frame ...>{ ... }</Frame>
    </ScrollView>
})
```

滚动输入由 Kernel 处理（`UiAction::Scroll`），应用无需手动计算偏移。

参考：`apps/win32-editor/src/presentation.rs`（编辑器页）。
