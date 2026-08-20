# Fragment

> 多子节点组合：不产生包裹节点，把多个子输出（`ViewOutput`）拼进父容器。无 Props。

## 示例

```rust
fn section(build: &mut ViewBuild<A>) -> ViewResult<ViewOutput<A>> {
    ui!(build {
        <Fragment>
            <Text value={"标题"} />
            <Text value={"正文"} />
        </Fragment>
    })
}

// 父容器内直接内联子函数输出（等价组合，常用形态）：
ui!(build {
    <Column ...>
        { section(build) }
        { nav_button(build, Route::About, "关于", route) }
    </Column>
})
```

子函数返回 `ViewResult<ViewOutput<A>>` 后以 `{ ... }` 内联，是拆分组件的标准方式（见 `apps/win32-editor/src/presentation.rs` 的 `top_bar`/`nav_button`）。
