# Text

> 文本原语。内容二选一：`value={...}` 属性或单个 `{ expr }` 子表达式（不能同时用）。公共属性见 [README](README.md)。

## Props

| 属性 | 类型 | 默认 | 说明 |
|---|---|---|---|
| `value` | `impl Into<String>` | 必填（或子表达式） | 文本内容 |
| `font` | `TextStyleRef` | `body()` | 字体样式引用 |
| `font_size` | `f32` | `14.0` | 字号 |
| `line_height` | `f32` | `20.0` | 行高 |
| `color` | `Color` | `BLACK` | 文本颜色 |

## 示例

```rust
// 属性形式
<Text value={format!("{label}: {value}")} font_size={14.0} color={SECONDARY} />

// 子表达式形式（等价）
<Text>{format!("{label}: {value}")}</Text>

// 动态字号/行高（随设置 Signal）
<Text value={document.to_owned()}
      font_size={settings.font_size as f32}
      line_height={font_size * (settings.line_height as f32 / 100.0)}
      color={TEXT} />
```

参考：`apps/win32-editor/src/presentation.rs`、`crates/ui/dsl/tests/ui_macro.rs`。
