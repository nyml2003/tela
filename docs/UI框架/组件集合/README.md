# 组件集合（ui! DSL）

> **状态：📖 使用参考。** `ui!` 声明式 UI DSL 的全部组件与 Props。用法与集成见（待写：总览 / 动作与信号 / 文本输入 / 应用集成 / 样式与布局）。类型来自 `tela-contract`；属性由 `tela-ui-dsl-macros` 校验（不支持会编译报错）。

## 公共属性（全部节点共享）

| 属性 | 类型 | 默认 | 说明 |
|---|---|---|---|
| `key` | `impl Into<String>` | 自动 | SemanticKey，**树内唯一**（重复 → `DuplicateKey` 构建失败） |
| `width` / `height` | `Option<Size>` | `None` | `Size::fixed(f32)` 固定 / `Size::percent(f32)` 百分比 |
| `margin` / `padding` | `Insets` | zero | 外边距 / 内边距 |
| `border_width` | `f32` | `0.0` | 边框宽度（border-box，计入宽高） |
| `gap` | `f32` | `0.0` | 子节点间距（容器） |
| `cross_align` | `CrossAlign` | `Start` | Row/Column 交叉轴对齐 |
| `clip` | `bool` | `false` | 裁剪子节点 |
| `overflow` | `Overflow` | `Visible` | 溢出行为（ScrollView 用 `Scroll`） |
| `grid_item` | `Option<GridItemPlacement>` | `None` | Grid 直接子项的显式位置 |
| `text_constraint` | `Option<TextConstraint>` | `None` | 文本行数/省略约束 |
| `fill` | `Option<Fill>` | `None` | 填充：`Fill::Solid(Color)` / `Linear` / `Radial` |
| `border_color` | `Option<Color>` | `None` | 边框颜色 |
| `border_radius` | `BorderRadius` | `0` | 四角圆角 |
| `shadow` | `Option<ShadowSpec>` | `None` | 阴影 |
| `draw_order` | `DrawOrder` | 默认 | 父容器内的绘制/命中顺序 |
| `visual_offset` | `PixelOffset` | zero | 不改变布局的视觉位移 |
| `clickable` / `hoverable` / `focusable` | `bool` | `false` | 交互槽位 |
| `View` 特有 | — | — | **允许空子节点**；`Frame` 必须恰好 1 子 |
| `tab_index` | `i16` | `0` | 焦点序（`-1` 移出 Tab 序列） |
| `input` | `Option<TextInputSpec>` | `None` | 文本输入语义（`TextInputSpec::new(TextInputKind::Multiline)` 等多行/单行） |
| `bind_id` | `impl Into<String>` | `None` | 业务绑定：`ValueChange` 唯一通道 |
| `pointer_capture` | `bool` | `false` | 按下后捕获 PointerId |
| `gestures` | `GestureConfig` | default | 通用手势申请 |
| `modal` | `bool` | `false` | 模态节点拦截下层输入 |

## 组件索引

| 组件 | 用途 | 专属 Props |
|---|---|---|
| [Column](Column.md) | 垂直容器 | 无 |
| [Row](Row.md) | 水平容器 | 无 |
| [Frame](Frame.md) | 单子节点尺寸边界（交互/视觉槽位），**必须有子节点** | 无 |
| [View](View.md) | 通用盒模型容器，**可空**（空 = 装饰块），0..1 子 | 无 |
| [Stack](Stack.md) | 层叠容器（子节点同框叠放） | 无 |
| [ScrollView](ScrollView.md) | 滚动容器 | 无 |
| [Text](Text.md) | 文本 | `value` `font` `font_size` `line_height` `color` |
| [Icon](Icon.md) | 图标（字体字形） | 同 Text（font 默认 icon） |
| [Image](Image.md) | 图片（纹理引用） | `texture`（必填） |
| [ActionTarget](ActionTarget.md) | 交互目标：点击/文本动作注册 | `action` `on_input` `on_submit` `on_cancel` |
| [For](For.md) | 静态列表循环 | `each` `key`（均必填） |
| [VirtualList](VirtualList.md) | 虚拟化列表 | `items` `total_items` `key` `item_height` `item_spacing` `overscan` `first_item_index` |
| [Fragment](Fragment.md) | 多子节点组合（无包裹节点） | 无 |

## 最小示例

```rust
use tela_ui_dsl::{ViewBuild, ViewOutput, ViewResult, ui};
use tela_contract::{Color, Fill, Size};

ui!(build {
    <Frame
        key={"demo.root"}
        width={Size::fixed(320.0)}
        height={Size::fixed(200.0)}
        fill={Fill::Solid(Color::WHITE)}
        clickable={true}
    >
        <Text value={"Hello Tela"} font_size={16.0} color={Color::BLACK} />
    </Frame>
})
```

返回值 `ViewResult<ViewOutput<A>>`：错误传播用 `?`；多个输出可组合（`Fragment`、子函数返回后内联）。
