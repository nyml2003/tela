//! `#[memo]` 组件的普通 props 必须实现 `PartialEq`，否则指纹无法生成。

use std::marker::PhantomData;

use tela_ui_dsl::prelude::*;
use tela_ui_dsl::{Body, DslComponent, ViewBuild, ViewOutput, ViewResult, ui};

/// 故意不可比较的 props 类型。
#[derive(Clone, Default)]
struct Opaque {
    _marker: PhantomData<*const ()>,
}

#[derive(DslComponent)]
#[memo]
struct MemoPanel {
    value: Opaque,
}

impl MemoPanel {
    fn view<A>(&self, build: &mut ViewBuild<A>, _children: Body<A>) -> ViewResult<ViewOutput<A>> {
        ui!(build {
            <Text value={"x"} />
        })
    }
}

fn main() {}
