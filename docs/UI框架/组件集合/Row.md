# Row

> 水平容器：子节点沿主轴（横向）依次排列。公共属性见 [README](README.md)。

## Props

仅公共属性（`gap` 控制子间距，`cross_align` 控制垂直对齐）。

## 示例

```rust
ui!(build {
    <Row
        key={"win32.topbar"}
        width={Size::fixed(width)}
        height={Size::fixed(40.0)}
        padding={Insets { top: 0.0, right: 0.0, bottom: 0.0, left: 8.0 }}
        gap={4.0}
        cross_align={CrossAlign::Center}
        fill={Fill::Solid(BAR_BACKGROUND)}
        border_width={1.0}
        border_color={BAR_BORDER}
    >
        { nav_button(build, Route::Editor, "编辑器", route) }
        { nav_button(build, Route::Settings, "设置", route) }
    </Row>
})
```

参考：`apps/win32-editor/src/presentation.rs`（顶部导航栏）。
