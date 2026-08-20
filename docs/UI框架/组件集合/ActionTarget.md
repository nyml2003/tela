# ActionTarget

> 交互目标：把子节点注册为点击/文本动作的触发源。**不能携带 `@watch`/`@provide`/`@inject`**（放真实子节点上）；至少一个动作属性。公共属性**不适用**（ActionTarget 无布局/视觉槽位）。

## Props

| 属性 | 类型 | 说明 |
|---|---|---|
| `action` | 应用动作表达式 | 点击触发，如 `EditorAction::Navigate(Route::Settings)` |
| `on_input` | `TextActionMap` | 文本输入变化触发（`UiAction::ValueChange`） |
| `on_submit` | `TextActionMap` | 提交（Enter）触发 |
| `on_cancel` | `TextActionMap` | 取消触发 |

## 示例

```rust
ui!(build {
    <ActionTarget action={EditorAction::Navigate(Route::Settings)}>
        <Frame key={"btn.settings"} width={Size::fixed(72.0)} height={Size::fixed(30.0)}
               fill={Fill::Solid(BAR_BACKGROUND)} clickable={true}>
            <Text value={"设置"} />
        </Frame>
    </ActionTarget>
})
```

动作经 `FrameCoordinator` 分发到应用（`EditorAction` 枚举 → `handle_application_action`）。

参考：`apps/win32-editor/src/presentation.rs`（导航按钮、设置步进按钮）。
