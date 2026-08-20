# Icon

> 图标原语：以字体字形渲染（`TextStyleRef::icon()`）。Props 与 [Text](Text.md) 相同，仅 `font` 默认值为 `icon()`。公共属性见 [README](README.md)。

## Props

| 属性 | 类型 | 默认 | 说明 |
|---|---|---|---|
| `value` | `impl Into<String>` | 必填（或子表达式） | 图标码点/名称 |
| `font` | `TextStyleRef` | `icon()` | 图标字体样式 |
| `font_size` | `f32` | `14.0` | 图标尺寸 |
| `line_height` | `f32` | `20.0` | 行高 |
| `color` | `Color` | `BLACK` | 图标颜色 |

## 示例

```rust
ui!(build {
    <Icon value={"search"} font_size={24.0} color={PRIMARY} />
})
```

参考：`apps/mobile-demo/src/presentation.rs`（搜索图标）。
