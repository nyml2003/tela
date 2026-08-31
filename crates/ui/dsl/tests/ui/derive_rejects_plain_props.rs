//! derive 契约（001 §2）：组件输入只能是 `#[watch]` 的 Signal/Computed 边或 `key`。
//! 普通值 props 是未声明的暗通道（父级传入的变化对失效不可见），编译期拒绝。

use tela_ui_dsl::{Children, DslComponent, ViewBuild, ViewOutput, ViewResult, ui};

#[derive(DslComponent)]
struct LegacyPanel {
    width: f32,
}

impl LegacyPanel {
    fn view<A>(
        &self,
        build: &mut ViewBuild<A>,
        _children: &Children<'_, A>,
    ) -> ViewResult<ViewOutput<A>> {
        ui!(build {
            <Text value={"legacy"} />
        })
    }
}

fn main() {}
