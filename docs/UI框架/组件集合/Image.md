# Image

> 图片原语：按纹理引用渲染。**不能有子节点**；`texture` 必填。公共属性见 [README](README.md)。

## Props

| 属性 | 类型 | 默认 | 说明 |
|---|---|---|---|
| `texture` | `impl Into<String>` | **必填** | 纹理引用（资源集内注册的纹理名） |
| 公共属性 | — | — | 尺寸/填充/交互等 |

## 示例

```rust
ui!(build {
    <Image key={"cover"} texture={"cover.png"} width={Size::fixed(96.0)} height={Size::fixed(96.0)} />
})
```
