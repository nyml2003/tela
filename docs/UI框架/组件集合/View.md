# View

> 通用盒模型容器：允许 **0..1 个内容子节点**（与 [Frame](Frame.md) 的唯一区别是**可空**）。空 View = 纯装饰块（分隔线/色块/背景层）；单子 View = 通用盒子。公共属性见 [README](README.md)。

## Props

仅公共属性。

## 语义

| 形态 | 行为 |
|---|---|
| `<View />`（空） | 纯装饰块：尺寸 + 填充/边框/圆角，无内容 |
| `<View>...</View>`（单子） | 通用盒子：与 Frame 相同（盒模型 + 视觉 + 交互槽位） |
| 双子及以上 | 编译/构建报错（`InvalidLayoutShape`）——多子用 `Column`/`Row`/`Stack` |

## 示例

```rust
// 分隔线（空 View）
<View key={"divider"}
      width={Size::fixed(viewport.width - 32.0)}
      height={Size::fixed(1.0)}
      fill={Fill::Solid(BAR_BORDER)} />

// 通用盒子（单子）
<View key={"card"} width={Size::fixed(280.0)} height={Size::fixed(64.0)}
      fill={Fill::Solid(Color::WHITE)} border_width={1.0} border_color={BAR_BORDER}>
    <Text value={"卡片内容"} />
</View>
```

参考：`apps/win32-editor/src/presentation.rs`（设置页分隔线）。
